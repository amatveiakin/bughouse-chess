// TODO: Switch from JSON to a binary format.
// TODO: Move all serialization/deserialization from bughouse_wasm/src/lib.rs here.

use std::io;
use std::net::TcpStream;

use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use serde::{de, Serialize};
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

pub const PORT: u16 = 14361;

#[derive(Debug)]
pub enum CommunicationError {
    ConnectionClosed,
    Socket(tungstenite::Error),
    Serde(serde_json::Error),
    BughouseProtocol(String),
}

pub fn write_obj<T, S>(socket: &mut WebSocket<S>, obj: &T) -> Result<(), CommunicationError>
where
    T: Serialize,
    S: io::Read + io::Write,
{
    let serialized = serde_json::to_string(obj).map_err(CommunicationError::Serde)?;
    socket
        .write_message(Message::Text(serialized))
        .map_err(CommunicationError::Socket)
}

pub fn read_obj<T, S>(socket: &mut WebSocket<S>) -> Result<T, CommunicationError>
where
    T: de::DeserializeOwned,
    S: io::Read + io::Write,
{
    let msg = socket.read_message().map_err(CommunicationError::Socket)?;
    match msg {
        Message::Text(msg) => serde_json::from_str(&msg).map_err(CommunicationError::Serde),
        Message::Close(_) => Err(CommunicationError::ConnectionClosed),
        _ => Err(CommunicationError::BughouseProtocol(format!("Expected text, got {:?}", msg))),
    }
}

pub async fn write_obj_async<T, S>(socket: &mut S, obj: &T) -> Result<(), CommunicationError>
where
    T: Serialize,
    S: SinkExt<Message, Error = tungstenite::Error> + Unpin,
{
    let serialized = serde_json::to_string(obj).map_err(CommunicationError::Serde)?;
    socket.send(Message::Text(serialized)).await.map_err(CommunicationError::Socket)
}

pub async fn read_obj_async<T, S>(socket: &mut S) -> Result<T, CommunicationError>
where
    T: de::DeserializeOwned,
    S: StreamExt<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    let msg = socket.next().await.map_or(Err(CommunicationError::ConnectionClosed), |m| {
        m.map_err(CommunicationError::Socket)
    })?;
    match msg {
        Message::Text(msg) => serde_json::from_str(&msg).map_err(CommunicationError::Serde),
        Message::Close(_) => Err(CommunicationError::ConnectionClosed),
        _ => Err(CommunicationError::BughouseProtocol(format!("Expected text, got {:?}", msg))),
    }
}

// TODO: Instead of cloning the socket, consider calling TcpStream.set_nonblocking on the
//   underlying stream and doing read/writes in the same thread.
pub fn clone_websocket(socket: &WebSocket<TcpStream>, role: Role) -> WebSocket<TcpStream> {
    let stream = socket.get_ref().try_clone().unwrap();
    let config = *socket.get_config();
    WebSocket::from_raw_socket(stream, role, Some(config))
}
