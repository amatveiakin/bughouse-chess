// TODO: Move SQL writes to a separate thread (https://crates.io/crates/sqlx should
//   do this automatically).
// TODO: Cache compiled SQL queries (should be easy with https://crates.io/crates/sqlx).
// TODO: Benchmark SQL write speed.
// TODO: More structured way to map data between Rust types and SQL;
//   consider https://crates.io/crates/sea-orm.
// TODO: insert a consistent row id, not to rely on implementation-specific
//   columns, such as ROWID in sqlite.

use log::error;
use sqlx::prelude::*;
use time::OffsetDateTime;

use bughouse_chess::persistence::*;
use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::*;

pub struct SqlxServerHooks<DB: sqlx::Database> {
    invocation_id: String,
    pool: sqlx::Pool<DB>,
}

impl SqlxServerHooks<sqlx::Sqlite>
{
    pub fn new(address: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(address)
            .create_if_missing(true);
        let pool =
            async_std::task::block_on(sqlx::SqlitePool::connect_with(options))?;
        Self::create_tables(&pool, "")?;
        Ok(Self {
            invocation_id: uuid::Uuid::new_v4().to_string(),
            pool,
        })
    }
}

impl SqlxServerHooks<sqlx::Postgres>
{
    pub fn new(address: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let pool =
            async_std::task::block_on(sqlx::Pool::<sqlx::Postgres>::connect(&format!("{address}")))?;
        Self::create_tables(&pool, "rowid BIGSERIAL PRIMARY KEY,")?;
        Ok(Self {
            invocation_id: uuid::Uuid::new_v4().to_string(),
            pool,
        })
    }
}

impl<DB: sqlx::Database> ServerHooks for SqlxServerHooks<DB>
where
    String: Type<DB> + for<'q>  Encode<'q, DB>,
    i64: Type<DB> + for<'q> Encode<'q, DB>,
    Option<i64>: Type<DB> + for<'q> Encode<'q, DB>,
    Option<OffsetDateTime>: Type<DB> + for<'q> Encode<'q, DB>,
    bool: Type<DB> + for<'q> Encode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
{
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
        self.record_game_finish(event, maybe_game, round);
    }
}

impl<DB: sqlx::Database> SqlxServerHooks<DB>
where
    String: Type<DB> + for<'q>  Encode<'q, DB>,
    i64: Type<DB> + for<'q> Encode<'q, DB>,
    Option<i64>: Type<DB> + for<'q> Encode<'q, DB>,
    Option<OffsetDateTime>: Type<DB> + for<'q> Encode<'q, DB>,
    bool: Type<DB> + for<'q> Encode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
{
    fn create_tables(pool: &sqlx::Pool<DB>, rowid_column_definition: &str) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Include contest_id in finished_games.
        async_std::task::block_on(
            sqlx::query(
                format!(
                    "CREATE TABLE IF NOT EXISTS finished_games (
                    {rowid_column_definition}
                    git_version TEXT,
                    invocation_id TEXT,
                    game_start_time TIMESTAMP,
                    game_end_time TIMESTAMP,
                    player_red_a TEXT,
                    player_red_b TEXT,
                    player_blue_a TEXT,
                    player_blue_b TEXT,
                    result TEXT,
                    game_pgn TEXT,
                    rated BOOLEAN DEFAULT TRUE)",
                ).as_str()
            )
            .execute(pool),
        )?;
        async_std::task::block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS client_performance (
                git_version TEXT,
                invocation_id TEXT,
                user_agent TEXT,
                time_zone TEXT,
                ping_p50 INTEGER,
                ping_p90 INTEGER,
                ping_p99 INTEGER,
                ping_n INTEGER,
                turn_confirmation_p50 INTEGER,
                turn_confirmation_p90 INTEGER,
                turn_confirmation_p99 INTEGER,
                turn_confirmation_n INTEGER,
                process_outgoing_events_p99 INTEGER,
                process_notable_events_p99 INTEGER,
                refresh_p99 INTEGER,
                update_state_p50 INTEGER,
                update_state_p90 INTEGER,
                update_state_p99 INTEGER,
                update_state_n INTEGER,
                update_clock_p99 INTEGER,
                update_drag_state_p99 INTEGER)",
            )
            .execute(pool))?;
        Ok(())
    }
    fn record_game_finish(
        &mut self,
        event: &BughouseServerEvent,
        maybe_game: Option<&GameState>,
        round: usize,
    ) {
        let Some(row) = self.game_result(event, maybe_game, round) else {
            return;
        };
        let execute_result = async_std::task::block_on(
            sqlx::query(
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
                game_pgn,
                rated)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(row.git_version)
            .bind(row.invocation_id)
            .bind(row.game_start_time)
            .bind(row.game_end_time)
            .bind(row.player_red_a)
            .bind(row.player_red_b)
            .bind(row.player_blue_a)
            .bind(row.player_blue_b)
            .bind(row.result)
            .bind(row.game_pgn)
            .bind(row.rated)
            .execute(&self.pool),
        );
        if let Err(e) = execute_result {
            error!("Error persisting game result: {}", e);
        }
    }

    fn record_client_performance(&mut self, perf: &BughouseClientPerformance) {
        let stats = &perf.stats;
        let ping = stats.get("ping");
        let turn_confirmation = stats.get("turn_confirmation");
        let process_outgoing_events = stats.get("process_outgoing_events");
        let process_notable_events = stats.get("process_notable_events");
        let refresh = stats.get("refresh");
        let update_state = stats.get("update_state");
        let update_clock = stats.get("update_clock");
        let update_drag_state = stats.get("update_drag_state");
        let execute_result = async_std::task::block_on(sqlx::query(
            "INSERT INTO client_performance (
                git_version,
                invocation_id,
                user_agent,
                time_zone,
                ping_p50,
                ping_p90,
                ping_p99,
                ping_n,
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
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)")
                .bind(my_git_version!().to_owned())
                .bind(self.invocation_id.clone())
                .bind(perf.user_agent.clone())
                .bind(perf.time_zone.clone())
                .bind(ping.map(|s| s.p50 as i64))
                .bind(ping.map(|s| s.p90 as i64))
                .bind(ping.map(|s| s.p99 as i64))
                .bind(ping.map(|s| s.num_values as i64))
                .bind(turn_confirmation.map(|s| s.p50 as i64))
                .bind(turn_confirmation.map(|s| s.p90 as i64))
                .bind(turn_confirmation.map(|s| s.p99 as i64))
                .bind(turn_confirmation.map(|s| s.num_values as i64))
                .bind(process_outgoing_events.map(|s| s.p99 as i64))
                .bind(process_notable_events.map(|s| s.p99 as i64))
                .bind(refresh.map(|s| s.p99 as i64))
                .bind(update_state.map(|s| s.p50 as i64))
                .bind(update_state.map(|s| s.p90 as i64))
                .bind(update_state.map(|s| s.p99 as i64))
                .bind(update_state.map(|s| s.num_values as i64))
                .bind(update_clock.map(|s| s.p99 as i64))
                .bind(update_drag_state.map(|s| s.p99 as i64))
            .execute(&self.pool));
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
                (players(game), game_result_str(*game_status)?)
            }
            BughouseServerEvent::GameOver { game_status, .. } => {
                (players(game), game_result_str(*game_status)?)
            }
            _ => {
                return None;
            }
        };
        Some(GameResultRow {
            git_version: my_git_version!().to_owned(),
            invocation_id: self.invocation_id.to_string(),
            game_start_time: game.start_offset_time(),
            game_end_time: Some(time::OffsetDateTime::now_utc()),
            player_red_a: players.0,
            player_red_b: players.1,
            player_blue_a: players.2,
            player_blue_b: players.3,
            result,
            game_pgn: pgn::export_to_bpgn(pgn::BughouseExportFormat {}, game.game(), round),
            rated: game.rated(),
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

fn players(game: &GameState) -> (String, String, String, String) {
    let get_player = |team, board_idx| {
        game.game().board(board_idx).player_name(get_bughouse_force(team, board_idx)).to_owned()
    };
    (
        get_player(Team::Red, BughouseBoard::A),
        get_player(Team::Red, BughouseBoard::B),
        get_player(Team::Blue, BughouseBoard::A),
        get_player(Team::Blue, BughouseBoard::B),
    )
}
