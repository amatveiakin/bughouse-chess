// Improvement potential. Try to do everything via message-passing, without `Mutex`es,
//   but also witout threading and network logic inside `ServerState`.
//   Problem. Adding client via event is a potential race condition in case the
//   first TCP message from the client arrives earlier.

use std::net::TcpListener;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use regex::Regex;
use tungstenite::protocol;

use bughouse_chess::*;
use bughouse_chess::server::*;

use crate::network;


pub struct ServerConfig {
    pub starting_time: String,
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

// Improvement potential: Better error handling.
pub fn run(config: ServerConfig) {
    let chess_rules = ChessRules {
        starting_position: StartingPosition::FischerRandom,
        time_control: TimeControl {
            starting_time: parse_starting_time(&config.starting_time)
        },
    };
    let bughouse_rules = BughouseRules {
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
    let clients_adder = Arc::clone(&clients);
    thread::spawn(move || {
        let mut server_state = ServerState::new(clients, chess_rules, bughouse_rules);
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
                let peer_addr = stream.peer_addr().unwrap();
                println!("Client connected: {}", peer_addr);
                let mut socket_in = tungstenite::accept(stream).unwrap();
                let mut socket_out = network::clone_websocket(&socket_in, protocol::Role::Server);
                let (client_tx, client_rx) = mpsc::channel();
                let client_id = clients_adder.lock().unwrap().add_client(client_tx, peer_addr.to_string());
                let tx_new = tx.clone();
                let clients_remover1 = Arc::clone(&clients_adder);
                let clients_remover2 = Arc::clone(&clients_adder);
                // Rust-upgrade (https://github.com/rust-lang/rust/issues/90470):
                //   Use `JoinHandle.is_running` in order to join the read/write threads in a
                //   non-blocking way.
                thread::spawn(move || {
                    loop {
                        match network::read_obj(&mut socket_in) {
                            Ok(ev) => {
                                tx_new.send(IncomingEvent::Network(client_id, ev)).unwrap();
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
            }
            Err(err) => {
                println!("Cannot establish connection: {}", err);
            }
        }
    }
    panic!("Unexpected end of TcpListener::incoming");
}
