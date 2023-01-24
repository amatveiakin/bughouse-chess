// Improvement potential. Try to do everything via message-passing, without `Mutex`es,
//   but also witout threading and network logic inside `ServerState`.
//   Problem. Adding client via event is a potential race condition in case the
//   first TCP message from the client arrives earlier.

use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use async_tungstenite::WebSocketStream;
use futures_io::{AsyncRead, AsyncWrite};
use futures_util::{sink::SinkExt, stream::StreamExt};
use log::{error, info, warn};
use serde::Deserialize;
use tungstenite::protocol;

use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::*;

use crate::network::{self, CommunicationError};
use crate::server_main::{DatabaseOptions, ServerConfig};
use crate::session::Session;
use crate::sqlx_server_hooks::*;

fn to_debug_string<T: std::fmt::Debug>(v: T) -> String {
    format!("{v:?}")
}

async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static>(
    peer_addr: String,
    mut stream: WebSocketStream<S>,
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
    // Calling blocking functions (such as client_tx.recv()) within async context
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
        async_std::task::block_on(done_tx.send(()));
    });
    // This instead of just running the loop to completion or join() on the
    // thread for the same reason of not blocking the async executor thread.
    done_rx.recv().await.unwrap();
    Ok(())
}

pub fn run(config: ServerConfig) {
    let (tx, rx) = mpsc::sync_channel(1000);
    let tx_tick = tx.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(100));
        tx_tick.send(IncomingEvent::Tick).unwrap();
    });
    let clients = Arc::new(Mutex::new(Clients::new()));
    let clients_copy = Arc::clone(&clients);

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

    const OAUTH_CSRF_COOKIE_NAME: &str = "oauth-csrf-state";

    let mut app = tide::with_state(
        crate::auth::GoogleAuth::new(crate::auth::Config {
            callback_url: "http://localhost:14361/session".to_owned(),
        })
        .unwrap(),
    );

    app.with(tide::sessions::SessionMiddleware::new(
        tide::sessions::CookieStore::new(),
        b"we recommend you use std::env::var(\"TIDE_SECRET\").unwrap().as_bytes() instead of a fixed value"
    ));

    app.with(tide::utils::After(|mut res: tide::Response| async {
        if let Some(err) = res.error() {
            let msg = format!("Error: {:?}", err);
            res.set_status(err.status());
            res.set_body(msg);
        }
        Ok(res)
    }));

    app.at("/login").get(
        |mut req: tide::Request<crate::auth::GoogleAuth>| async move {
            let session = req.session();
            let mut session_data = match session.get::<Session>("data") {
                Some(d) => {
                    if d.logged_in {
                        return Ok(format!(
                            "You are already logged in. UserInfo: \n{:?}",
                            d.user_info
                        )
                        .into());
                    }
                    d
                }
                None => Session::default(),
            };
            let (redirect_url, csrf_state) = req.state().start()?;
            req.session_mut().insert("data", session_data);

            let mut resp: tide::Response = req.into();
            resp.set_status(tide::StatusCode::TemporaryRedirect);
            resp.insert_header(http_types::headers::LOCATION, redirect_url.as_str());

            // Using a separate cookie for oauth csrf state because the session
            // cookie has SameSite::Strict policy (and we want to keep that),
            // which prevents browsers from setting the session cookie upon
            // redirect.
            // This will use the default, which is SameSite::Lax on most browsers,
            // which should still be good enough.
            let mut csrf_cookie = http_types::cookies::Cookie::new(
                OAUTH_CSRF_COOKIE_NAME,
                csrf_state.secret().to_owned(),
            );
            csrf_cookie.set_http_only(true);
            resp.insert_cookie(csrf_cookie);
            Ok(resp)
        },
    );

    app.at("/session").get(
        |mut req: tide::Request<crate::auth::GoogleAuth>| async move {
            let session = req.session();
            let mut session_data = match session.get::<Session>("data") {
                Some(d) => {
                    if d.logged_in {
                        return Ok(format!(
                            "You are already logged in. UserInfo: \n{:?}",
                            d.user_info
                        ));
                    }
                    d
                }
                None => Session::default(),
            };
            let (auth_code, request_csrf_state) =
                req.query::<crate::auth::NewSessionQuery>()?.parse();
            let Some(oauth_csrf_state_cookie) = req.cookie(OAUTH_CSRF_COOKIE_NAME) else {
                return Err(tide::Error::from_str(
                    403, "Missing CSRF token cookie.",
                ));
            };
            if oauth_csrf_state_cookie.value() != request_csrf_state.secret() {
                return Err(tide::Error::from_str(403, "Non-matching CSRF token."));
            }
            let user_info = req.state().user_info(auth_code).await?;
            session_data.user_info = user_info.clone();
            req.session_mut().insert("data", session_data);
            Ok(format!(
                "You are now logged in. UserInfo: \n{:?}",
                user_info
            ))
        },
    );

    app.at("/logout")
        .get(|mut req: tide::Request<_>| async move {
            let session = req.session_mut();
            session.remove("data");
            Ok("You are now logged out.")
        });

    app.at("/mysession")
        .get(|req: tide::Request<_>| async move {
            let session = req.session();
            Ok(format!("{:?}", session.get::<Session>("data")))
        });

    app.at("/").get(move |req: tide::Request<_>| {
        let mytx = tx.clone();
        let myclients = clients.clone();
        async move {
            let peer_addr = req.peer_addr().map_or_else(
                || {
                    Err(tide::Error::new(
                        403,
                        anyhow::Error::msg("Peer address missing"),
                    ))
                },
                |x| Ok(x.to_owned()),
            )?;
            let session = req.session().clone();

            // tide::Request -> http_types::Request -> http::Request<Body> -> http::Request<()>.
            let http_types_req: http_types::Request = req.into();
            let http_req_with_body: http::Request<http_types::Body> = http_types_req.into();
            let http_req = http_req_with_body.map(|_| ());

            let http_resp = tungstenite::handshake::server::create_response(&http_req)
                .map_err(|e| tide::Error::new(400, e))?;

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
                        handle_connection(peer_addr, stream, mytx, myclients, session.get("data"))
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
    async_std::task::block_on(async { app.listen(format!("0.0.0.0:{}", network::PORT)).await })
        .expect("Failed to start the tide server");
}