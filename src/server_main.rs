use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use bughouse_chess::*;
use bughouse_chess::server::*;


pub fn run() {
    let (tx, rx) = mpsc::channel();
    let tx_client_connected = tx.clone();
    let tx_tick = tx.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            tx_tick.send(IncomingEvent::Tick).unwrap();
        }
    });
    thread::spawn(move || {
        let mut server_state = ServerState::new(tx);
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
                tx_client_connected.send(IncomingEvent::ClientConnected(stream)).unwrap();
            }
            Err(err) => {
                println!("Cannot estanblish connection: {}", err);
            }
        }
    }
    panic!("Unexpected end of TcpListener::incoming");
}
