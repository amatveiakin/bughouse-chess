use std::collections::HashSet;

use async_trait::async_trait;

use crate::event::{BughouseClientPerformance, FinishedGameDescription};
use crate::game::BughouseGame;
use crate::utc_time::UtcDateTime;


#[async_trait]
pub trait ServerHooks {
    async fn record_client_performance(&self, perf: &BughouseClientPerformance);
    async fn record_finished_game(
        &self, game: &BughouseGame, registered_users: &HashSet<String>,
        game_start_time: UtcDateTime, game_end_time: UtcDateTime, round: u64,
    );
    async fn get_games_by_user(
        &self, user_name: &str,
    ) -> Result<Vec<FinishedGameDescription>, String>;
    async fn get_game_bpgn(&self, game_id: i64) -> Result<String, String>;
}

pub struct NoopServerHooks {}

#[async_trait]
impl ServerHooks for NoopServerHooks {
    async fn record_client_performance(&self, _perf: &BughouseClientPerformance) {}
    async fn record_finished_game(
        &self, _game: &BughouseGame, _registered_users: &HashSet<String>,
        _game_start_time: UtcDateTime, _game_end_time: UtcDateTime, _round: u64,
    ) {
    }
    async fn get_games_by_user(
        &self, _user_name: &str,
    ) -> Result<Vec<FinishedGameDescription>, String> {
        Err("Server hooks not available".to_owned())
    }
    async fn get_game_bpgn(&self, _game_id: i64) -> Result<String, String> {
        Err("Server hooks not available".to_owned())
    }
}
