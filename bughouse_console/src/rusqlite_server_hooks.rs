use log::error;

use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::*;


pub struct RusqliteServerHooks {
    invocation_id: String,
    game_start_time: Option<chrono::DateTime<chrono::Utc>>,
    conn: rusqlite::Connection,
}

impl RusqliteServerHooks {
    pub fn new(address: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(address)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS finished_games (
                git_version TEXT,
                invocation_id TEXT,
                game_start_time TIMESTAMP,
                game_end_time TIMESTAMP,
                player_red_a TEXT,
                player_red_b TEXT,
                player_blue_a TEXT,
                player_blue_b TEXT,
                result TEXT,
                game_pgn TEXT)",
            (),
        )?;
        Ok(Self {
            invocation_id: uuid::Uuid::new_v4().to_string(),
            game_start_time: None,
            conn,
        })
    }
}

impl ServerHooks for RusqliteServerHooks {
    fn on_event(
        &mut self,
        event: &BughouseServerEvent,
        maybe_game: Option<&GameState>,
        round: usize,
    ) {
        if let BughouseServerEvent::GameStarted { .. } = event {
            self.game_start_time = Some(chrono::offset::Utc::now());
        }
        self.record_game_finish(event, maybe_game, round);
    }
}

impl RusqliteServerHooks {
    fn record_game_finish(
        &mut self,
        event: &BughouseServerEvent,
        maybe_game: Option<&GameState>,
        round: usize,
    ) -> Option<()> {
        if let Some(row) = self.game_result(event, maybe_game, round) {
            let execute_result = self.conn.execute(
                "INSERT INTO finished_games (
                    git_version,
                    invocation_id,
                    game_start_time,
                    game_end_time,
                    player_red_a,
                    player_red_b,
                    player_blue_a,
                    player_blue_b,
                    result,
                    game_pgn)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (
                    row.git_version,
                    row.invocation_id,
                    row.game_start_time,
                    row.game_end_time,
                    row.player_red_a,
                    row.player_red_b,
                    row.player_blue_a,
                    row.player_blue_b,
                    row.result,
                    row.game_pgn,
                ),
            );
            if let Err(e) = execute_result {
                error!("Error persisting game result: {:?}", e);
                None
            } else {
                Some(())
            }
        } else {
            None
        }
    }

    fn game_result(
        &self,
        event: &BughouseServerEvent,
        maybe_game: Option<&GameState>,
        round: usize,
    ) -> Option<GameResultRow> {
        let game = maybe_game?;
        let (players, result) = match event {
            BughouseServerEvent::TurnsMade { game_status, .. } => {
                (players(game)?, game_result_str(*game_status)?)
            }
            BughouseServerEvent::GameOver { game_status, .. } => {
                (players(game)?, game_result_str(*game_status)?)
            }
            _ => {
                return None;
            }
        };
        Some(GameResultRow {
            git_version: my_git_version!().to_owned(),
            invocation_id: self.invocation_id.to_string(),
            game_start_time: self.game_start_time.map(|x| x.timestamp()),
            game_end_time: Some(chrono::offset::Utc::now().timestamp()),
            player_red_a: players.0,
            player_red_b: players.1,
            player_blue_a: players.2,
            player_blue_b: players.3,
            result,
            game_pgn: game.bpgn(pgn::BughouseExportFormat{}, round),
        })
    }
}

#[derive(Debug)]
struct GameResultRow {
    git_version: String,
    invocation_id: String,
    game_start_time: Option<i64>,
    game_end_time: Option<i64>,
    player_red_a: String,
    player_red_b: String,
    player_blue_a: String,
    player_blue_b: String,
    result: String,
    game_pgn: String,
}

fn game_result_str(status: BughouseGameStatus) -> Option<String> {
    match status {
        BughouseGameStatus::Victory(Team::Red, _) => Some("VICTORY_RED"),
        BughouseGameStatus::Victory(Team::Blue, _) => Some("VICTORY_BLUE"),
        BughouseGameStatus::Draw(_) => Some("DRAW"),
        BughouseGameStatus::Active => None,
    }
    .map(|x| x.to_owned())
}

fn players(game: &GameState) -> Option<(String, String, String, String)> {
    let mut red_a = None;
    let mut red_b = None;
    let mut blue_a = None;
    let mut blue_b = None;
    for (player, board) in game.players_with_boards().iter() {
        match (player.team, board) {
            (Team::Red, BughouseBoard::A) => {
                red_a = Some(player.name.clone());
            }
            (Team::Red, BughouseBoard::B) => {
                red_b = Some(player.name.clone());
            }
            (Team::Blue, BughouseBoard::A) => {
                blue_a = Some(player.name.clone());
            }
            (Team::Blue, BughouseBoard::B) => {
                blue_b = Some(player.name.clone());
            }
        }
    }
    Some((red_a?, red_b?, blue_a?, blue_b?))
}
