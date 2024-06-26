use std::ops::Range;

use tide::utils::async_trait;
use time::OffsetDateTime;

use crate::bughouse_prelude::*;
use crate::client_performance_stats::ClientPerformanceRecord;
use crate::competitor::Competitor;

#[derive(Debug)]
pub struct GameResultRow {
    pub git_version: String,
    pub invocation_id: String,
    pub game_start_time: Option<OffsetDateTime>,
    pub game_end_time: Option<OffsetDateTime>,
    pub player_red_a: Competitor,
    pub player_red_b: Competitor,
    pub player_blue_a: Competitor,
    pub player_blue_b: Competitor,
    pub result: String,
    pub game_pgn: String,
    pub rated: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct RowId {
    pub id: i64,
}

#[async_trait]
pub trait DatabaseReader {
    async fn finished_games(
        &self, game_end_time_range: Range<OffsetDateTime>, only_rated: bool,
    ) -> Result<Vec<(RowId, GameResultRow)>, anyhow::Error>;
    async fn pgn(&self, rowid: RowId) -> Result<String, anyhow::Error>;
    async fn client_performance(&self) -> Result<Vec<ClientPerformanceRecord>, anyhow::Error>;
}

#[async_trait]
pub trait DatabaseWriter {
    async fn create_tables(&self) -> anyhow::Result<()>;
    async fn add_finished_game(&self, row: GameResultRow) -> anyhow::Result<()>;
    async fn add_client_performance(
        &self, perf: &BughouseClientPerformance, invocation_id: &str,
    ) -> anyhow::Result<()>;
}
