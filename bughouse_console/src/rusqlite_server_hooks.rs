// TODO: Move SQL writes to a separate thread (https://crates.io/crates/sqlx should
//   do this automatically).
// TODO: Cache compiled SQL queries (should be easy with https://crates.io/crates/sqlx).
// TODO: Benchmark SQL write speed.
// TODO: More structured way to map data between Rust types and SQL;
//   consider https://crates.io/crates/sea-orm.

use log::error;

use bughouse_chess::persistence::*;
use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::*;

pub struct RusqliteServerHooks {
    invocation_id: String,
    game_start_time: Option<time::OffsetDateTime>,
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
        conn.execute(
            "CREATE TABLE IF NOT EXISTS client_performance (
                git_version TEXT,
                invocation_id TEXT,
                user_agent TEXT,
                time_zone TEXT,
                turn_confirmation_p50,
                turn_confirmation_p90,
                turn_confirmation_p99,
                turn_confirmation_n,
                process_outgoing_events_p99,
                process_notable_events_p99,
                refresh_p99,
                update_state_p50,
                update_state_p90,
                update_state_p99,
                update_state_n,
                update_clock_p99,
                update_drag_state_p99)",
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
    fn on_client_event(&mut self, event: &BughouseClientEvent) {
        if let BughouseClientEvent::ReportPerformace(performance) = event {
            self.record_client_performance(&performance)
        }
    }
    fn on_server_broadcast_event(
        &mut self,
        event: &BughouseServerEvent,
        maybe_game: Option<&GameState>,
        round: usize,
    ) {
        if let BughouseServerEvent::GameStarted { .. } = event {
            self.game_start_time = Some(time::OffsetDateTime::now_utc());
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
    ) {
        let Some(row) = self.game_result(event, maybe_game, round) else {
            return;
        };
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
            error!("Error persisting game result: {}", e);
        }
    }

    fn record_client_performance(&mut self, perf: &BughouseClientPerformance) {
        let stats = &perf.stats;
        let turn_confirmation = stats.get("turn_confirmation");
        let process_outgoing_events = stats.get("process_outgoing_events");
        let process_notable_events = stats.get("process_notable_events");
        let refresh = stats.get("refresh");
        let update_state = stats.get("update_state");
        let update_clock = stats.get("update_clock");
        let update_drag_state = stats.get("update_drag_state");
        let execute_result = self.conn.execute(
            "INSERT INTO client_performance (
                git_version,
                invocation_id,
                user_agent,
                time_zone,
                turn_confirmation_p50,
                turn_confirmation_p90,
                turn_confirmation_p99,
                turn_confirmation_n,
                process_outgoing_events_p99,
                process_notable_events_p99,
                refresh_p99,
                update_state_p50,
                update_state_p90,
                update_state_p99,
                update_state_n,
                update_clock_p99,
                update_drag_state_p99)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                my_git_version!().to_owned(),
                self.invocation_id,
                perf.user_agent,
                perf.time_zone,
                turn_confirmation.map(|s| s.p50),
                turn_confirmation.map(|s| s.p90),
                turn_confirmation.map(|s| s.p99),
                turn_confirmation.map(|s| s.num_values),
                process_outgoing_events.map(|s| s.p99),
                process_notable_events.map(|s| s.p99),
                refresh.map(|s| s.p99),
                update_state.map(|s| s.p50),
                update_state.map(|s| s.p90),
                update_state.map(|s| s.p99),
                update_state.map(|s| s.num_values),
                update_clock.map(|s| s.p99),
                update_drag_state.map(|s| s.p99),
            ]
        );
        if let Err(e) = execute_result {
            error!("Error persisting client performance: {}", e);
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
            game_start_time: self.game_start_time.map(|x| x.unix_timestamp()),
            game_end_time: Some(time::OffsetDateTime::now_utc().unix_timestamp()),
            player_red_a: players.0,
            player_red_b: players.1,
            player_blue_a: players.2,
            player_blue_b: players.3,
            result,
            game_pgn: pgn::export_to_bpgn(pgn::BughouseExportFormat{}, game.game(), round),
        })
    }
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
