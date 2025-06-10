// TODO: The server somewhat haphazardly mixes async and non-async synchronization primitives.
//   We should figure out a proper concurrency story: either transition fully to async code, or
//   get systematic about how we use threads vs async tasks.

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_std::channel;
use async_std::sync::Mutex;
use async_tungstenite::WebSocketStream;
use bughouse_chess::event::BughouseServerEvent;
use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::session::Session;
use bughouse_chess::session_store::*;
use bughouse_chess::utc_time::UtcDateTime;
use futures_io::{AsyncRead, AsyncWrite};
use futures_util::StreamExt;
use instant::Instant;
use log::{error, info, warn};
use prometheus::Encoder;
use tide::StatusCode;
use tide_jsx::html;
use time::OffsetDateTime;
use tungstenite::protocol;

use crate::auth_handlers_tide::*;
use crate::database_server_hooks::*;
use crate::http_server_state::*;
use crate::network::{self, CommunicationError};
use crate::persistence::DatabaseReader;
use crate::prod_server_helpers::ProdServerHelpers;
use crate::secret_persistence::SecretDatabaseRW;
use crate::server_config::{AuthOptions, DatabaseOptions, ServerConfig, SessionOptions};
use crate::{auth, database};

async fn handle_connection<
    DB: Sync + Send + 'static,
    S: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
>(
    peer_addr: String, stream: WebSocketStream<S>, tx: channel::Sender<IncomingEvent>,
    clients: Arc<Clients>, session_id: Option<SessionId>, http_server_state: HttpServerState<DB>,
) -> tide::Result<()> {
    let (mut stream_tx, mut stream_rx) = stream.split();
    info!("Client connected: {}", peer_addr);

    let (client_tx, client_rx) = async_std::channel::unbounded();

    let session_store_subscription_id = if let Some(session_id) = &session_id {
        // Subscribe the client to all updates to the session in session store.
        let my_client_tx = client_tx.clone();
        Some(http_server_state.session_store.lock().await.subscribe(session_id, move |s| {
            // Send the entire session data to the client.
            // We can perform some mapping here if we want to hide
            // some of the state from the client.
            let _ =
                my_client_tx.try_send(BughouseServerEvent::UpdateSession { session: s.clone() });
        }))
    } else {
        None
    };

    let client_id = clients.add_client(client_tx, session_id.clone(), peer_addr.to_string()).await;
    let clients_remover1 = ClientRemover {
        client_id,
        session_id,
        session_store_subscription_id,
        clients,
        session_store: Arc::clone(&http_server_state.session_store),
    };
    let clients_remover2 = clients_remover1.clone();

    // Client -> Server
    async_std::task::spawn(async move {
        loop {
            match network::read_obj_async(&mut stream_rx).await {
                Ok(ev) => {
                    tx.send(IncomingEvent::Network(client_id, ev)).await.unwrap();
                }
                Err(err) => {
                    if let Some(logging_id) = clients_remover1.remove().await {
                        match err {
                            CommunicationError::ConnectionClosed => {
                                info!("Client {} disconnected", logging_id)
                            }
                            err => warn!(
                                "Client {} disconnected due to read error: {:?}",
                                logging_id, err
                            ),
                        }
                    }
                    break;
                }
            }
        }
    });

    // Server -> Client
    while let Ok(ev) = client_rx.recv().await {
        match network::write_obj_async(&mut stream_tx, &ev).await {
            Ok(()) => {}
            Err(err) => {
                if let Some(logging_id) = clients_remover2.remove().await {
                    warn!("Client {} disconnected due to write error: {:?}", logging_id, err);
                }
                break;
            }
        }
    }

    Ok(())
}

// Converts an `http_types::Request` into an `http::Request<()>`.
//
// This conversion is necessary because `tide` (and `http-types`) uses an older version
// of the `http` crate's ecosystem (`http` v0.2.x) than `tungstenite` (`http` v1.x)
// since the `f3cf3538` dependency update.
// Previously, this conversion was handled automatically via the `Into` trait.
fn http_types_req_to_http_req(
    http_types_req: http_types::Request,
) -> tide::Result<http::Request<()>> {
    let version = match http_types_req.version() {
        Some(http_types::Version::Http0_9) => http::Version::HTTP_09,
        Some(http_types::Version::Http1_0) => http::Version::HTTP_10,
        Some(http_types::Version::Http1_1) => http::Version::HTTP_11,
        Some(http_types::Version::Http2_0) => http::Version::HTTP_2,
        Some(http_types::Version::Http3_0) => http::Version::HTTP_3,
        None => http::Version::HTTP_11,
        Some(v) => {
            return Err(tide::Error::new(
                StatusCode::MethodNotAllowed,
                anyhow!("Invalid version: {:?}", v),
            ));
        }
    };
    let mut builder = http::Request::builder()
        .method(http_types_req.method().to_string().as_str())
        .uri(http_types_req.url().as_str())
        .version(version);
    for (name, values) in &http_types_req {
        for value in values.iter() {
            builder = builder.header(name.as_str(), value.as_str());
        }
    }
    builder
        .body(())
        .map_err(|e| tide::Error::new(StatusCode::InternalServerError, e))
}

// Converts an `http::Response<http_types::Body>` into an `http_types::Response`.
//
// This conversion is necessary because `tide` (and `http-types`) uses an older version
// of the `http` crate's ecosystem (`http` v0.2.x) than `tungstenite` (`http` v1.x)
// since the `f3cf3538` dependency update.
// Previously, this conversion was handled automatically via the `Into` trait.
fn http_resp_to_http_types_resp(
    http_resp: http::Response<http_types::Body>,
) -> tide::Result<http_types::Response> {
    let (parts, body) = http_resp.into_parts();
    let mut http_types_resp = http_types::Response::new(
        http_types::StatusCode::try_from(parts.status.as_u16())
            .map_err(|e| tide::Error::new(StatusCode::InternalServerError, e.into_inner()))?,
    );
    for (name, value) in &parts.headers {
        if let Ok(value_str) = value.to_str() {
            http_types_resp.append_header(name.as_str(), value_str);
        }
    }
    http_types_resp.set_body(body);
    Ok(http_types_resp)
}

async fn run_tide<DB: Sync + Send + 'static + DatabaseReader>(
    config: ServerConfig, db: DB, secret_db: Box<dyn SecretDatabaseRW>,
    session_store: Arc<Mutex<SessionStore>>, clients: Arc<Clients>,
    server_info: Arc<Mutex<ServerInfo>>, tx: channel::Sender<IncomingEvent>,
) {
    let (auth_callback_is_https, google_auth, lichess_auth) = match config.auth_options {
        None => (false, None, None),
        Some(AuthOptions { callback_is_https, google, lichess }) => (
            callback_is_https,
            google.map(|ga| {
                auth::GoogleAuth::new(ga.client_id_source, ga.client_secret_source).unwrap()
            }),
            lichess.map(|la| auth::LichessAuth::new(la.client_id_source).unwrap()),
        ),
    };
    let mut app = tide::with_state(Arc::new(HttpServerStateImpl {
        sessions_enabled: config.session_options != SessionOptions::NoSessions,
        google_auth,
        lichess_auth,
        auth_callback_is_https,
        db,
        secret_db,
        static_content_url_prefix: config.static_content_url_prefix,
        session_store,
        server_info,
    }));

    if let SessionOptions::WithSessions { secret, expire_in } = config.session_options {
        app.with(
            tide::sessions::SessionMiddleware::new(
                tide::sessions::CookieStore::new(),
                secret.get().unwrap().as_bytes(),
            )
            // Set to Lax so that the session persists third-party
            // redirects like Google Auth.
            .with_same_site_policy(http_types::cookies::SameSite::Lax)
            .with_session_ttl(Some(expire_in)),
        );
    }

    app.with(tide::utils::After(|mut res: tide::Response| async {
        if let Some(err) = res.error() {
            let msg = format!("Error: {:#?}", err);
            res.set_status(err.status());
            res.set_body(msg);
        }
        Ok(res)
    }));

    app.at(AUTH_SIGNUP_PATH).post(handle_signup);
    app.at(AUTH_LOGIN_PATH).post(handle_login);
    app.at(AUTH_LOGOUT_PATH).post(handle_logout);
    app.at(AUTH_SIGN_WITH_GOOGLE_PATH).get(handle_sign_with_google);
    app.at(AUTH_SIGN_WITH_LICHESS_PATH).get(handle_sign_with_lichess);
    app.at(AUTH_CONTINUE_SIGN_WITH_GOOGLE_PATH)
        .get(handle_continue_sign_with_google);
    app.at(AUTH_CONTINUE_SIGN_WITH_LICHESS_PATH)
        .get(handle_continue_sign_with_lichess);
    app.at(AUTH_FINISH_SIGNUP_WITH_GOOGLE_PATH)
        .post(handle_finish_signup_with_google);
    app.at(AUTH_FINISH_SIGNUP_WITH_LICHESS_PATH)
        .post(handle_finish_signup_with_lichess);
    app.at(AUTH_CHANGE_ACCOUNT_PATH).post(handle_change_account);
    app.at(AUTH_DELETE_ACCOUNT_PATH).post(handle_delete_account);
    app.at(AUTH_MYSESSION_PATH).get(handle_mysession);
    app.at(AUTH_CHECK_PLAYER_NAME).post(handle_check_player_name);
    app.at(AUTH_CHECK_NEW_USER_NAME).post(handle_check_new_user_name);

    app.at("/dyn/metrics").get(handle_metrics);
    app.at("/dyn/server").get(handle_server_info);

    crate::stats_handlers_tide::Handlers::<HttpServerState<DB>>::register_handlers(&mut app);

    let allowed_origin = config.allowed_origin;

    app.at("/").get(move |req: tide::Request<HttpServerState<_>>| {
        let mytx = tx.clone();
        let myclients = clients.clone();
        let allowed_origin = allowed_origin.clone();
        async move {
            if req.state().sessions_enabled {
                // When the sessions are enabled, we might be using the session
                // cookie for authentication.
                // We should be checking request origin in that case to
                // preven CSRF, to which websockets are inherently vulnerable.
                check_origin(&req, &allowed_origin)?;
            }
            let peer_addr = req.peer_addr().map_or_else(
                || Err(tide::Error::from_str(StatusCode::Forbidden, "Peer address missing")),
                |x| Ok(x.to_owned()),
            )?;


            let http_server_state = req.state().clone();

            let session_id = get_session_id(&req).ok();

            // This will renew the expiration time on all the sessions.
            // We only call this when the user accesses "/", not on every single
            // interaction with the websocket for simplicity and performance.
            // Sessions where a user has a tab open throughout expiration time
            // are probably not something we want anyway.
            if let Some(session_id) = &session_id {
                http_server_state.session_store.lock().await.touch(session_id).await;
            }

            // tide::Request -> http_types::Request -> http::Request<Body> -> http::Request<()>.
            let http_types_req: http_types::Request = req.into();
            let http_req = http_types_req_to_http_req(http_types_req)?;

            let http_resp = tungstenite::handshake::server::create_response(&http_req)
                .map_err(|e| tide::Error::new(StatusCode::BadRequest, e))?;

            // http::Response<()> -> http::Response<Body> -> http_types::Response
            let http_resp_with_body = http_resp.map(|_| http_types::Body::empty());
            let mut http_types_resp = http_resp_to_http_types_resp(http_resp_with_body)?;

            // http_types::Response is a magic thing that can give us the stream back
            // once it's upgraded.
            let upgrade_receiver = http_types_resp.recv_upgrade().await;

            async_std::task::spawn(async move {
                if let Some(stream) = upgrade_receiver.await {
                    let stream =
                        WebSocketStream::from_raw_socket(stream, protocol::Role::Server, None)
                            .await;
                    if let Err(err) = handle_connection(
                        peer_addr,
                        stream,
                        mytx,
                        myclients,
                        session_id,
                        http_server_state,
                    )
                    .await
                    {
                        error!("{}", err);
                    }
                } else {
                    error!("Never received an upgrade for client {}", peer_addr);
                }
            });
            Ok(http_types_resp)
        }
    });
    app.listen(format!("0.0.0.0:{}", network::PORT))
        .await
        .expect("Failed to start the tide server");
}

fn sessions_required(config: &ServerConfig) -> bool {
    let Some(o) = config.auth_options.as_ref() else {
        return false;
    };
    o.google.is_some() || o.lichess.is_some()
}

pub async fn run(config: ServerConfig) {
    assert!(
        !sessions_required(&config) || config.session_options != SessionOptions::NoSessions,
        "Authentication is enabled while sessions are not."
    );

    let options = ServerOptions {
        check_git_version: config.check_git_version,
        max_starting_time: config.max_starting_time,
    };

    // Limited buffer for data streaming from clients into the server.
    // When this is full because ServerState::apply_event isn't coping with
    // the load, we start putting back pressure on client websockets.
    let (tx, rx) = channel::bounded(100000);
    let tx_tick = tx.clone();
    let tx_terminate = tx.clone();
    let server_info = Arc::new(Mutex::new(ServerInfo::new()));
    let server_info_copy = Arc::clone(&server_info);
    let clients = Arc::new(Clients::new(&options));
    let clients_copy = Arc::clone(&clients);

    ctrlc::set_handler(move || {
        async_std::task::block_on(async {
            tx_terminate.send(IncomingEvent::Terminate).await.unwrap()
        })
    })
    .expect("Error setting Ctrl-C handler");

    async_std::task::spawn(async move {
        loop {
            async_std::task::sleep(Duration::from_millis(100)).await;
            tx_tick.send(IncomingEvent::Tick).await.unwrap();
        }
    });

    let hooks = match config.database_options.clone() {
        DatabaseOptions::NoDatabase => None,
        DatabaseOptions::Sqlite(address) => {
            let db = database::SqlxDatabase::<sqlx::Sqlite>::new(&address)
                .await
                .unwrap_or_else(|_| panic!("Cannot connect to SQLite DB {address}"));
            let h = DatabaseServerHooks::new(db).await.expect("Cannot initialize hooks");
            Some(Arc::new(h) as Arc<dyn ServerHooks + Send + Sync>)
        }

        DatabaseOptions::Postgres(address) => {
            let db = database::SqlxDatabase::<sqlx::Postgres>::new(&address)
                .await
                .unwrap_or_else(|_| panic!("Cannot connect to Postgres DB {address}"));
            let h = DatabaseServerHooks::new(db).await.expect("Cannot initialize hooks");
            Some(Arc::new(h) as Arc<dyn ServerHooks + Send + Sync>)
        }
    };

    let secret_database = make_database(&config.secret_database_options).await.unwrap();
    let _ = secret_database.create_tables().await.map_err(|err| {
        error!("Failed to create tables: {}", err);
        // Proceed even if table creation failed.
    });

    let session_store = Arc::new(Mutex::new(SessionStore::new()));

    if let SessionOptions::WithSessions { expire_in, .. } = &config.session_options {
        let expire_in = *expire_in;

        // It's OK to instantiate a separate connection.
        let secret_database_for_sessions: Arc<dyn SecretDatabaseRW> =
            make_database(&config.secret_database_options).await.unwrap().into();

        if let Err(e) = secret_database_for_sessions.gc_expired_sessions(expire_in).await {
            error!("Failed to GC expired sessions: {}", e);
        }
        if let Err(e) =
            restore_sessions(secret_database_for_sessions.as_ref(), &mut session_store.lock().await)
                .await
        {
            error!("Failed to restore sessions: {}", e);
            // Proceed even if restoring sessions failed.
        }
        session_store.lock().await.on_any_change(move |session_id, session: Session| {
            let secret_database_for_sessions = Arc::clone(&secret_database_for_sessions);
            Box::pin(async move {
                if let Err(e) = secret_database_for_sessions
                    .set_logged_in_session(
                        &session_id,
                        session.user_info().map(|i| i.user_name.clone()),
                        OffsetDateTime::now_utc(),
                    )
                    .await
                {
                    error!("Failed to persist session info: {}", e);
                }
            })
        });
    }

    let session_store_copy = Arc::clone(&session_store);
    async_std::task::spawn(async move {
        let mut server_state = ServerState::new(
            options,
            clients_copy,
            session_store_copy,
            server_info_copy,
            Arc::new(ProdServerHelpers {}),
            hooks,
        );

        loop {
            let event = rx.recv().await.unwrap();
            server_state.apply_event(event, Instant::now(), UtcDateTime::now()).await;
        }
    });

    match config.database_options.clone() {
        DatabaseOptions::NoDatabase => {
            run_tide(
                config,
                database::UnimplementedDatabase {},
                secret_database,
                session_store,
                clients,
                server_info,
                tx,
            )
            .await
        }
        DatabaseOptions::Sqlite(address) => {
            run_tide(
                config,
                database::SqlxDatabase::<sqlx::Sqlite>::new(&address).await.unwrap(),
                secret_database,
                session_store,
                clients,
                server_info,
                tx,
            )
            .await
        }
        DatabaseOptions::Postgres(address) => {
            run_tide(
                config,
                database::SqlxDatabase::<sqlx::Postgres>::new(&address).await.unwrap(),
                secret_database,
                session_store,
                clients,
                server_info,
                tx,
            )
            .await
        }
    }
}

// Replacement for an async closure because those are not clonable.
#[derive(Clone)]
struct ClientRemover {
    client_id: ClientId,
    session_id: Option<SessionId>,
    session_store_subscription_id: Option<SubscriptionId>,
    clients: Arc<Clients>,
    session_store: Arc<Mutex<SessionStore>>,
}
impl ClientRemover {
    async fn remove(self) -> Option<String> {
        if let (Some(session_id), Some(session_store_subscription_id)) =
            (self.session_id, self.session_store_subscription_id)
        {
            self.session_store
                .lock()
                .await
                .unsubscribe(&session_id, session_store_subscription_id);
        }
        self.clients.remove_client(self.client_id).await
    }
}

async fn make_database(options: &DatabaseOptions) -> anyhow::Result<Box<dyn SecretDatabaseRW>> {
    Ok(match options {
        DatabaseOptions::NoDatabase => Box::new(database::UnimplementedDatabase {}),
        DatabaseOptions::Sqlite(address) => {
            Box::new(database::SqlxDatabase::<sqlx::Sqlite>::new(address).await?)
        }
        DatabaseOptions::Postgres(address) => {
            Box::new(database::SqlxDatabase::<sqlx::Postgres>::new(address).await?)
        }
    })
}

async fn restore_sessions(
    db: &dyn SecretDatabaseRW, session_store: &mut async_std::sync::MutexGuard<'_, SessionStore>,
) -> anyhow::Result<()> {
    let sessions = db.list_sessions().await?;
    for (id, value) in sessions {
        session_store.set(id, value).await;
    }
    Ok(())
}

// TODO: Add Prometheus config to git.
// TODO: Add instructions on Prometheus and Grafana.
async fn handle_metrics<DB>(_req: tide::Request<HttpServerState<DB>>) -> tide::Result {
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
    let body = String::from_utf8(buffer.clone()).unwrap();
    let mut resp = tide::Response::new(StatusCode::Ok);
    resp.set_body(body);
    Ok(resp)
}

async fn handle_server_info<DB>(req: tide::Request<HttpServerState<DB>>) -> tide::Result {
    let info = req.state().server_info.lock().await.clone();
    let h: String = html! {
        <html>
        <head>
        </head>
        <body>
            {"Users online: "}{info.num_clients}<br/>
            {"Active matches: "}{info.num_active_matches}<br/>
        </body>
        </html>
    };
    let mut resp = tide::Response::new(StatusCode::Ok);
    resp.set_content_type(http_types::Mime::from("text/html; charset=UTF-8"));
    resp.set_body(h);
    Ok(resp)
}
