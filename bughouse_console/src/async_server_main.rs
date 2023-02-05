// Improvement potential. Try to do everything via message-passing, without `Mutex`es,
//   but also witout threading and network logic inside `ServerState`.
//   Problem. Adding client via event is a potential race condition in case the
//   first TCP message from the client arrives earlier.

use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use async_tungstenite::WebSocketStream;
use futures_io::{AsyncRead, AsyncWrite};
use futures_util::StreamExt;
use log::{error, info, warn};
use rand::RngCore;
use tide::StatusCode;
use tungstenite::protocol;

use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;

use crate::auth_handlers_tide::*;
use crate::http_server_state::*;
use crate::network::{self, CommunicationError};
use crate::server_main::{AuthOptions, DatabaseOptions, ServerConfig, SessionOptions};
use crate::session::Session;
use crate::sqlx_server_hooks::*;

async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static>(
    peer_addr: String,
    stream: WebSocketStream<S>,
    tx: mpsc::SyncSender<IncomingEvent>,
    clients: Arc<Mutex<Clients>>,
    session: Option<Session>,
) -> tide::Result<()> {
    let (mut stream_tx, mut stream_rx) = stream.split();
    info!("Client connected: {}, session={:?}", peer_addr, session);

    let (client_tx, client_rx) = mpsc::channel();
    let client_id = clients
        .lock()
        .unwrap()
        .add_client(client_tx, peer_addr.to_string());
    let clients_remover1 = Arc::clone(&clients);
    let clients_remover2 = Arc::clone(&clients);
    async_std::task::spawn(async move {
        loop {
            match network::read_obj_async(&mut stream_rx).await {
                Ok(ev) => {
                    tx.send(IncomingEvent::Network(client_id, ev)).unwrap();
                }
                Err(err) => {
                    if let Some(logging_id) =
                        clients_remover1.lock().unwrap().remove_client(client_id)
                    {
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

    // Still spawning an OS thread here because client_rx is a
    // synchronous receiver.
    // Calling blocking functions (such as client_rx.recv()) within async context
    // means completely blocking an executor thread, which quickly leads to
    // stavation and deadlocks because the number of async executor threads
    // is limited.
    let (done_tx, done_rx) = async_std::channel::bounded(1);
    std::thread::spawn(move || {
        loop {
            let Ok(ev) = client_rx.recv() else { break };
            match async_std::task::block_on(network::write_obj_async(&mut stream_tx, &ev)) {
                Ok(()) => {}
                Err(err) => {
                    if let Some(logging_id) =
                        clients_remover2.lock().unwrap().remove_client(client_id)
                    {
                        warn!(
                            "Client {} disconnected due to write error: {:?}",
                            logging_id, err
                        );
                    }
                    break;
                }
            }
        }
        async_std::task::block_on(done_tx.send(())).unwrap();
    });
    // This instead of just running the loop to completion or join() on the
    // thread for the same reason of not blocking the async executor thread.
    done_rx.recv().await.unwrap();
    Ok(())
}

pub fn run(config: ServerConfig) {
    assert!(
        config.auth_options == AuthOptions::NoAuth
            || config.session_options != SessionOptions::NoSessions,
        "Authentication is enabled while sessions are not."
    );

    // Limited buffer for data streaming from clients into the server.
    // When this is full because ServerState::apply_event isn't coping with
    // the load, we start putting back pressure on client websockets.
    let (tx, rx) = mpsc::sync_channel(100000);
    let tx_tick = tx.clone();
    let tx_terminate = tx.clone();
    let clients = Arc::new(Mutex::new(Clients::new()));
    let clients_copy = Arc::clone(&clients);

    ctrlc::set_handler(move || tx_terminate.send(IncomingEvent::Terminate).unwrap())
        .expect("Error setting Ctrl-C handler");

    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(100));
        tx_tick.send(IncomingEvent::Tick).unwrap();
    });

    thread::spawn(move || {
        let hooks = match config.database_options {
            DatabaseOptions::NoDatabase => None,
            DatabaseOptions::Sqlite(address) => Some(Box::new(
                SqlxServerHooks::<sqlx::Sqlite>::new(address.as_str())
                    .unwrap_or_else(|err| panic!("Cannot connect to SQLite DB {address}:\n{err}")),
            ) as Box<dyn ServerHooks>),
            DatabaseOptions::Postgres(address) => Some(Box::new(
                SqlxServerHooks::<sqlx::Postgres>::new(address.as_str()).unwrap_or_else(|err| {
                    panic!("Cannot connect to Postgres DB {address}:\n{err}")
                }),
            ) as Box<dyn ServerHooks>),
        };
        let mut server_state = ServerState::new(clients_copy, hooks);

        for event in rx {
            server_state.apply_event(event);
        }
        panic!("Unexpected end of events stream");
    });

    let (google_auth, auth_callback_is_https) = match config.auth_options {
        AuthOptions::NoAuth => (None, false),
        AuthOptions::GoogleAuthFromEnv { callback_is_https } => (
            Some(crate::auth::GoogleAuth::new().unwrap()),
            callback_is_https,
        ),
    };

    let mut app = tide::with_state(Arc::new(HttpServerStateImpl {
        sessions_enabled: config.session_options != SessionOptions::NoSessions,
        google_auth,
        auth_callback_is_https,
    }));

    if app.state().sessions_enabled {
        let secret = match config.session_options {
            SessionOptions::NoSessions => unreachable!(),
            SessionOptions::WithNewRandomSecret => {
                let mut result = vec![0u8; 32];
                rand::thread_rng().fill_bytes(result.as_mut_slice());
                result
            }
            SessionOptions::WithSecret(secret) => secret,
        };
        app.with(tide::sessions::SessionMiddleware::new(
            tide::sessions::CookieStore::new(),
            secret.as_slice(),
        ));
    }

    app.with(tide::utils::After(|mut res: tide::Response| async {
        if let Some(err) = res.error() {
            let msg = format!("Error: {:#?}", err);
            res.set_status(err.status());
            res.set_body(msg);
        }
        Ok(res)
    }));

    app.at(AUTH_LOGIN_URL_PATH).get(handle_login);
    app.at(AUTH_SESSION_URL_PATH).get(handle_session);
    app.at(AUTH_LOGOUT_URL_PATH).get(handle_logout);
    app.at(AUTH_MYSESSION_URL_PATH).get(handle_mysession);

    app.at("/").get(move |req: tide::Request<HttpServerState>| {
        let mytx = tx.clone();
        let myclients = clients.clone();
        async move {
            if req.state().sessions_enabled {
                // When the sessions are enabled, we might be using the session
                // cookie for authentication.
                // We should be checking request origin in that case to
                // preven CSRF, to which websockets are inherently vulnerable.
                check_origin(&req)?;
            }
            let peer_addr = req.peer_addr().map_or_else(
                || {
                    Err(tide::Error::from_str(
                        StatusCode::Forbidden,
                        "Peer address missing",
                    ))
                },
                |x| Ok(x.to_owned()),
            )?;

            let session_data = get_session(&req).ok().and_then(|s| s.get("data"));

            // tide::Request -> http_types::Request -> http::Request<Body> -> http::Request<()>.
            let http_types_req: http_types::Request = req.into();
            let http_req_with_body: http::Request<http_types::Body> = http_types_req.into();
            let http_req = http_req_with_body.map(|_| ());

            let http_resp = tungstenite::handshake::server::create_response(&http_req)
                .map_err(|e| tide::Error::new(StatusCode::BadRequest, e))?;

            // http::Response<()> -> http::Response<Body> -> http_types::Response
            let http_resp_with_body = http_resp.map(|_| http_types::Body::empty());
            let mut http_types_resp: http_types::Response = http_resp_with_body.into();

            // http_types::Response is a magic thing that can give us the stream back
            // once it's upgraded.
            let upgrade_receiver = http_types_resp.recv_upgrade().await;

            async_std::task::spawn(async move {
                if let Some(stream) = upgrade_receiver.await {
                    let stream =
                        WebSocketStream::from_raw_socket(stream, protocol::Role::Server, None)
                            .await;
                    if let Err(err) =
                        handle_connection(peer_addr, stream, mytx, myclients, session_data).await
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
    async_std::task::block_on(async { app.listen(format!("0.0.0.0:{}", network::PORT)).await })
        .expect("Failed to start the tide server");
}
