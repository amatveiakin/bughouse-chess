// Improvement potential. Try to do everything via message-passing, without `Mutex`es,
//   but also witout threading and network logic inside `ServerState`.
//   Problem. Adding client via event is a potential race condition in case the
//   first TCP message from the client arrives earlier.

use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use log::{info, warn};
use tungstenite::protocol;

use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::session_store::SessionStore;
use bughouse_chess::*;

use crate::database;
use crate::database_server_hooks::*;
use crate::network::{self, CommunicationError};
use crate::prod_server_helpers::ProdServerHelpers;

#[derive(Debug, Clone)]
pub enum DatabaseOptions {
    NoDatabase,
    Sqlite(String),
    Postgres(String),
}

#[derive(Debug, Eq, PartialEq)]
pub enum AuthOptions {
    NoAuth,
    GoogleAuthFromEnv { callback_is_https: bool },
}

#[derive(Debug, Eq, PartialEq)]
pub enum SessionOptions {
    NoSessions,

    // Sessions terminate on server termination.
    WithNewRandomSecret,

    // Allows for sessions that survive server restart.
    // TODO: Support persistent sessions.
    #[allow(dead_code)]
    WithSecret(Vec<u8>),
}

#[derive(Debug)]
pub struct ServerConfig {
    pub database_options: DatabaseOptions,
    pub secret_database_options: DatabaseOptions,
    pub auth_options: AuthOptions,
    pub session_options: SessionOptions,
    pub static_content_url_prefix: String,
}

fn to_debug_string<T: std::fmt::Debug>(v: T) -> String {
    format!("{v:?}")
}

fn handle_connection(
    stream: TcpStream,
    clients: &Arc<Mutex<Clients>>,
    tx: mpsc::Sender<IncomingEvent>,
) -> Result<(), String> {
    let peer_addr = stream.peer_addr().map_err(to_debug_string)?;
    info!("Client connected: {}", peer_addr);
    let mut socket_in = tungstenite::accept(stream).map_err(to_debug_string)?;
    let mut socket_out = network::clone_websocket(&socket_in, protocol::Role::Server);
    let (client_tx, client_rx) = mpsc::channel();
    let client_id = clients
        .lock()
        .unwrap()
        .add_client(client_tx, None, peer_addr.to_string());
    let clients_remover1 = Arc::clone(&clients);
    let clients_remover2 = Arc::clone(&clients);
    // Rust-upgrade (https://github.com/rust-lang/rust/issues/90470):
    //   Use `JoinHandle.is_running` in order to join the read/write threads in a
    //   non-blocking way.
    thread::spawn(move || loop {
        match network::read_obj(&mut socket_in) {
            Ok(ev) => {
                tx.send(IncomingEvent::Network(client_id, ev)).unwrap();
            }
            Err(err) => {
                if let Some(logging_id) = clients_remover1.lock().unwrap().remove_client(client_id)
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
                return;
            }
        }
    });
    thread::spawn(move || {
        for ev in client_rx {
            match network::write_obj(&mut socket_out, &ev) {
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
                    return;
                }
            }
        }
    });
    Ok(())
}

pub fn run(config: ServerConfig) {
    assert_eq!(
        config.auth_options,
        AuthOptions::NoAuth,
        "Auth is not supported by this server implementation."
    );
    assert_eq!(
        config.session_options,
        SessionOptions::NoSessions,
        "Sessions are not supported by this server implementation."
    );

    let (tx, rx) = mpsc::channel();
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
        let hooks = match config.database_options.clone() {
            DatabaseOptions::NoDatabase => None,
            DatabaseOptions::Sqlite(address) => {
                let db = database::SqlxDatabase::<sqlx::Sqlite>::new(&address)
                    .expect(format!("Cannot connect to SQLite DB {address}").as_str());
                let h = DatabaseServerHooks::new(db).expect("Cannot initialize hooks");
                Some(Box::new(h) as Box<dyn ServerHooks>)
            }

            DatabaseOptions::Postgres(address) => {
                let db = database::SqlxDatabase::<sqlx::Postgres>::new(&address)
                    .expect(format!("Cannot connect to Postgres DB {address}").as_str());
                let h = DatabaseServerHooks::new(db).expect("Cannot initialize hooks");
                Some(Box::new(h) as Box<dyn ServerHooks>)
            }
        };
        let session_store = Arc::new(Mutex::new(SessionStore::new()));
        let mut server_state = ServerState::new(
            clients_copy,
            session_store,
            Box::new(ProdServerHelpers{}),
            hooks,
        );

        for event in rx {
            server_state.apply_event(event);
        }
        panic!("Unexpected end of events stream");
    });

    let listener = TcpListener::bind(("0.0.0.0", network::PORT)).unwrap();
    info!("Starting bughouse server version {}...", my_git_version!());
    info!(
        "Listening to connections on {}...",
        listener.local_addr().unwrap()
    );
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match handle_connection(stream, &clients, tx.clone()) {
                Ok(()) => {}
                Err(err) => {
                    warn!("{}", err);
                }
            },
            Err(err) => {
                warn!("Cannot establish connection: {}", err);
            }
        }
    }
    panic!("Unexpected end of TcpListener::incoming");
}
