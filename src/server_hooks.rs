use crate::event::BughouseClientPerformance;
use crate::game::BughouseGame;
use crate::utc_time::UtcDateTime;


pub trait ServerHooks {
    fn on_client_performance_report(&mut self, perf: &BughouseClientPerformance);
    fn on_game_over(
        &mut self, game: &BughouseGame, game_start_time: UtcDateTime, game_end_time: UtcDateTime,
        round: u64,
    );
}

pub struct NoopServerHooks {}

impl ServerHooks for NoopServerHooks {
    fn on_client_performance_report(&mut self, _perf: &BughouseClientPerformance) {}
    fn on_game_over(
        &mut self, _game: &BughouseGame, _game_start_time: UtcDateTime,
        _game_end_time: UtcDateTime, _round: u64,
    ) {
    }
}
