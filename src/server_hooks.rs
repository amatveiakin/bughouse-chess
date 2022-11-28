use crate::event::{BughouseClientEvent, BughouseServerEvent};
use crate::server::GameState;


pub trait ServerHooks {
    fn on_client_event(&mut self, event: &BughouseClientEvent);
    fn on_server_broadcast_event(&mut self, event: &BughouseServerEvent, game: Option<&GameState>, round: usize);
}

pub struct NoopServerHooks {}

impl ServerHooks for NoopServerHooks {
    fn on_client_event(&mut self, _event: &BughouseClientEvent) {}
    fn on_server_broadcast_event(&mut self, _event: &BughouseServerEvent, _game: Option<&GameState>, _round: usize) {}
}
