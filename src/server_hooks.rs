use crate::event::BughouseServerEvent;
use crate::server::*;

pub trait ServerHooks {
    fn on_event(&mut self, event: &BughouseServerEvent, game: Option<&GameState>, round: usize);
}

pub struct NoopServerHooks {}

impl ServerHooks for NoopServerHooks {
    fn on_event(&mut self, event: &BughouseServerEvent, game: Option<&GameState>, round: usize) {
    }
}
