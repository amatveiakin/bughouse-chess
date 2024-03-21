use crate::event::{BughouseClientPerformance, FinishedGameDescription};
use crate::game::BughouseGame;
use crate::utc_time::UtcDateTime;


// TODO: Don't allow hooks to block server! Especially for `get_games_by_user`: fetch response and
// return it when ready.
pub trait ServerHooks {
    fn on_client_performance_report(&mut self, perf: &BughouseClientPerformance);
    fn on_game_over(
        &mut self, game: &BughouseGame, game_start_time: UtcDateTime, game_end_time: UtcDateTime,
        round: u64,
    );
    fn get_games_by_user(&self, user_name: &str) -> Result<Vec<FinishedGameDescription>, String>;
}

pub struct NoopServerHooks {}

impl ServerHooks for NoopServerHooks {
    fn on_client_performance_report(&mut self, _perf: &BughouseClientPerformance) {}
    fn on_game_over(
        &mut self, _game: &BughouseGame, _game_start_time: UtcDateTime,
        _game_end_time: UtcDateTime, _round: u64,
    ) {
    }
    fn get_games_by_user(&self, _user_name: &str) -> Result<Vec<FinishedGameDescription>, String> {
        Err("Server hooks not available".to_owned())
    }
}
