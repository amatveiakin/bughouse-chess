use time::OffsetDateTime;

#[derive(Debug)]
pub struct GameResultRow {
    pub git_version: String,
    pub invocation_id: String,
    pub game_start_time: Option<OffsetDateTime>,
    pub game_end_time: Option<OffsetDateTime>,
    pub player_red_a: String,
    pub player_red_b: String,
    pub player_blue_a: String,
    pub player_blue_b: String,
    pub result: String,
    pub game_pgn: String,
    pub rated: bool,
}
