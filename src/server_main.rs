// Improvement potential. Try to do everything via message-passing, without `Mutex`es,
//   but also witout threading and network logic inside `ServerState`.
//   Problem. Adding client via event is a potential race condition in case the
//   first TCP message from the client arrives earlier.

use std::net::TcpListener;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use bughouse_chess::*;
use bughouse_chess::server::*;


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
                let mut in_stream = stream.try_clone().unwrap();
                let mut out_stream = stream;
                let (client_tx, client_rx) = mpsc::channel();
                let client_id = clients_adder.lock().unwrap().add_client(client_tx);
                let tx_new = tx.clone();
                let clients_remover = Arc::clone(&clients_adder);
                thread::spawn(move || {
                    loop {
                        let ev_data = network::read_str(&mut in_stream);
                        match ev_data {
                            Ok(ev_data) => {
                                let ev = network::parse_obj::<BughouseClientEvent>(&ev_data).unwrap();
                                tx_new.send(IncomingEvent::Network(client_id, ev)).unwrap();
                            },
                            Err(err) => {
                                use std::io::ErrorKind::*;
                                match err.kind() {
                                    ConnectionReset | ConnectionAborted => {
                                        println!("Client disconnected: {}", peer_addr);
                                        clients_remover.lock().unwrap().remove_client(client_id);
                                        // Rust-upgrade (https://github.com/rust-lang/rust/issues/90470):
                                        //   Use `JoinHandle.is_running` in order to join the thread in a
                                        //   non-blocking way.
                                        return;
                                    },
                                    _ => {
                                        panic!("Unexpected network error: {:?}", err);
                                    }
                                }
                            },
                        }
                    }
                });
                thread::spawn(move || {
                    for msg in client_rx {
                        network::write_obj(&mut out_stream, &msg).unwrap();
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
