// TODO: Switch from JSON to a binary format.
// TODO: Move all serialization/deserialization from bughouse_wasm/src/lib.rs here.

use std::io;
use std::net::TcpStream;

use serde::{de, Serialize};
use tungstenite::{WebSocket, Message, protocol::Role};


pub const PORT: u16 = 38617;


#[derive(Debug)]
pub enum CommunicationError {
    Socket(tungstenite::Error),
    Serde(serde_json::Error),
    BughouseProtocol(String),
}

pub fn write_obj<T, S>(socket: &mut WebSocket<S>, obj: &T) -> Result<(), CommunicationError>
where
    T: Serialize,
    S: io::Read + io::Write,
{
    let serialized = serde_json::to_string(obj).map_err(|err| CommunicationError::Serde(err))?;
    socket.write_message(Message::Text(serialized)).map_err(|err| CommunicationError::Socket(err))
}

pub fn read_obj<T, S>(socket: &mut WebSocket<S>) -> Result<T, CommunicationError>
where
    T: de::DeserializeOwned,
    S: io::Read + io::Write,
{
    let msg = socket.read_message().map_err(|err| CommunicationError::Socket(err))?;
    if let Message::Text(msg) = msg {
        serde_json::from_str(&msg).map_err(|err| CommunicationError::Serde(err))
    } else {
        Err(CommunicationError::BughouseProtocol(format!("Expected text, got {:?}", msg)))
    }
}


// TODO: Instead of cloning the socket, consider calling TcpStream.set_nonblocking on the
//   underlying stream and doing read/writes in the same thread.
pub fn clone_websocket(socket: &WebSocket<TcpStream>, role: Role) -> WebSocket<TcpStream> {
    let stream = socket.get_ref().try_clone().unwrap();
    let config = *socket.get_config();
    WebSocket::from_raw_socket(stream, role, Some(config))
}
