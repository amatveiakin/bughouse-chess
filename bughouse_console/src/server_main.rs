// Improvement potential. Try to do everything via message-passing, without `Mutex`es,
//   but also witout threading and network logic inside `ServerState`.
//   Problem. Adding client via event is a potential race condition in case the
//   first TCP message from the client arrives earlier.

use std::net::{TcpStream, TcpListener};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use regex::Regex;
use tungstenite::protocol;

use bughouse_chess::*;
use bughouse_chess::server::*;

use crate::network;


pub struct ServerConfig {
    pub teaming: String,
    pub starting_time: String,
}

fn to_debug_string<T: std::fmt::Debug>(v: T) -> String {
    format!("{v:?}")
}

fn parse_starting_time(time_str: &str) -> Duration {
    let time_re = Regex::new(r"([0-9]+):([0-9]{2})").unwrap();
    if let Some(cap) = time_re.captures(time_str) {
        let minutes = cap.get(1).unwrap().as_str().parse::<u64>().unwrap();
        let seconds = cap.get(2).unwrap().as_str().parse::<u64>().unwrap();
        Duration::from_secs(minutes * 60 + seconds)
    } else {
        panic!("Invalid starting time format: '{}', expected 'm:ss'", time_str);
    }
}

fn handle_connection(stream: TcpStream, clients: &Arc<Mutex<Clients>>, tx: mpsc::Sender<IncomingEvent>)
    -> Result<(), String>
{
    let peer_addr = stream.peer_addr().map_err(to_debug_string)?;
    println!("Client connected: {}", peer_addr);
    let mut socket_in = tungstenite::accept(stream).map_err(to_debug_string)?;
    let mut socket_out = network::clone_websocket(&socket_in, protocol::Role::Server);
    let (client_tx, client_rx) = mpsc::channel();
    let client_id = clients.lock().unwrap().add_client(client_tx, peer_addr.to_string());
    let clients_remover1 = Arc::clone(&clients);
    let clients_remover2 = Arc::clone(&clients);
    // Rust-upgrade (https://github.com/rust-lang/rust/issues/90470):
    //   Use `JoinHandle.is_running` in order to join the read/write threads in a
    //   non-blocking way.
    thread::spawn(move || {
        loop {
            match network::read_obj(&mut socket_in) {
                Ok(ev) => {
                    tx.send(IncomingEvent::Network(client_id, ev)).unwrap();
                },
                Err(err) => {
                    if let Some(logging_id) = clients_remover1.lock().unwrap().remove_client(client_id) {
                        println!("Client {} disconnected due to read error: {:?}", logging_id, err);
                    }
                    return;
                },
            }
        }
    });
    thread::spawn(move || {
        for ev in client_rx {
            match network::write_obj(&mut socket_out, &ev) {
                Ok(()) => {},
                Err(err) => {
                    if let Some(logging_id) = clients_remover2.lock().unwrap().remove_client(client_id) {
                        println!("Client {} disconnected due to write error: {:?}", logging_id, err);
                    }
                    return;
                },
            }
        }
    });
    Ok(())
}

pub fn run(config: ServerConfig) {
    let teaming = match config.teaming.as_str() {
        "fixed" => Teaming::FixedTeams,
        "dynamic" => Teaming::IndividualMode,
        other => panic!("Unexpected teaming: {}", other),
    };
    let chess_rules = ChessRules {
        starting_position: StartingPosition::FischerRandom,
        time_control: TimeControl {
            starting_time: parse_starting_time(&config.starting_time)
        },
    };
    let bughouse_rules = BughouseRules {
        teaming,
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(6),
        drop_aggression: DropAggression::NoChessMate,
    };

    let (tx, rx) = mpsc::channel();
    let tx_tick = tx.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            tx_tick.send(IncomingEvent::Tick).unwrap();
        }
    });
    let clients = Arc::new(Mutex::new(Clients::new()));
    let clients_copy = Arc::clone(&clients);
    thread::spawn(move || {
        let mut server_state = ServerState::new(clients_copy, chess_rules, bughouse_rules);
        for event in rx {
            server_state.apply_event(event);
        }
        panic!("Unexpected end of events stream");
    });

    let listener = TcpListener::bind(("0.0.0.0", network::PORT)).unwrap();
    println!("Listening to connection on {}...", listener.local_addr().unwrap());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                match handle_connection(stream, &clients, tx.clone()) {
                    Ok(()) => {},
                    Err(err) => {
                        println!("{}", err);
                    }
                }
            }
            Err(err) => {
                println!("Cannot establish connection: {}", err);
            }
        }
    }
    panic!("Unexpected end of TcpListener::incoming");
}
