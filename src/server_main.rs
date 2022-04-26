// TODO: Try to do everything via message-passing, without `Mutex`es, but also
//   witout threading and network logic inside `ServerState`.
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
    let clients_view = Arc::clone(&clients);
    thread::spawn(move || {
        let mut server_state = ServerState::new(clients);
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
                println!("Client connected from {}", stream.peer_addr().unwrap());
                let mut in_stream = stream.try_clone().unwrap();
                let mut out_stream = stream;
                let (client_tx, client_rx) = mpsc::channel();
                let client_id = clients_view.lock().unwrap().add_client(client_tx);
                let tx_new = tx.clone();
                thread::spawn(move || {
                    loop {
                        let ev = network::parse_obj::<BughouseClientEvent>(
                            &network::read_str(&mut in_stream).unwrap()).unwrap();
                        tx_new.send(IncomingEvent::Network(client_id, ev)).unwrap();
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
