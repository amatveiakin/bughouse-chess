// Improvement potential. Try to do everything via message-passing, without `Mutex`es,
//   but also witout threading and network logic inside `ServerState`.
//   Problem. Adding client via event is a potential race condition in case the
//   first TCP message from the client arrives earlier.

use std::net::TcpListener;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use tungstenite::protocol;

use bughouse_chess::*;
use bughouse_chess::server::*;


// Improvement potential: Better error handling.
pub fn run() {
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
        let chess_rules = ChessRules {
            starting_position: StartingPosition::FischerRandom,
            time_control: TimeControl{ starting_time: Duration::from_secs(300) },
        };
        let bughouse_rules = BughouseRules {
            min_pawn_drop_row: SubjectiveRow::from_one_based(2),
            max_pawn_drop_row: SubjectiveRow::from_one_based(6),
            drop_aggression: DropAggression::NoChessMate,
        };
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
                let client_id = clients_adder.lock().unwrap().add_client(client_tx);
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
                                println!("Client {} will be disconnected due to read error: {:?}", peer_addr, err);
                                clients_remover1.lock().unwrap().remove_client(client_id);
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
                                println!("Client {} will be disconnected due to write error: {:?}", peer_addr, err);
                                clients_remover2.lock().unwrap().remove_client(client_id);
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
