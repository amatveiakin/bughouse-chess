use crate::event::BughouseClientPerformance;
use crate::game::BughouseGame;


pub trait ServerHooks {
    fn on_client_performance_report(&mut self, perf: &BughouseClientPerformance);
    fn on_game_over(
        &mut self, game: &BughouseGame, game_start_offset_time: Option<time::OffsetDateTime>,
        round: usize,
    );
}

pub struct NoopServerHooks {}

impl ServerHooks for NoopServerHooks {
    fn on_client_performance_report(&mut self, _perf: &BughouseClientPerformance) {}
    fn on_game_over(
        &mut self, _game: &BughouseGame, _game_start_offset_time: Option<time::OffsetDateTime>,
        _round: usize,
    ) {
    }
}
