// TODO: Move SQL writes to a separate thread (https://crates.io/crates/sqlx should
//   do this automatically).
// TODO: Cache compiled SQL queries (should be easy with https://crates.io/crates/sqlx).
// TODO: Benchmark SQL write speed.
// TODO: More structured way to map data between Rust types and SQL;
//   consider https://crates.io/crates/sea-orm.
// TODO: insert a consistent row id, not to rely on implementation-specific
//   columns, such as ROWID in sqlite.
// TODO: streaming support + APIs.
use std::ops::Range;

use bughouse_chess::meter::MeterStats;
use bughouse_chess::my_git_version;
use log::error;
use sqlx::prelude::*;
use tide::utils::async_trait;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::bughouse_prelude::*;
use crate::client_performance_stats::ClientPerformanceRecord;
use crate::persistence::*;

trait U64AsI64Database {
    fn try_get_u64<'r, I>(&'r self, index: I) -> Result<u64, sqlx::Error>
    where
        I: sqlx::ColumnIndex<Self>;
}

impl<DB: sqlx::Database, R: sqlx::Row<Database = DB>> U64AsI64Database for R
where
    i64: Type<DB> + for<'q> Decode<'q, DB>,
{
    fn try_get_u64<'r, I>(&'r self, index: I) -> Result<u64, sqlx::Error>
    where
        I: sqlx::ColumnIndex<Self>,
    {
        self.try_get::<i64, _>(index).map(|x| x as u64)
    }
}

pub struct UnimplementedDatabase {}

#[async_trait]
impl DatabaseReader for UnimplementedDatabase {
    async fn finished_games(
        &self, _: Range<OffsetDateTime>, _: bool,
    ) -> Result<Vec<(RowId, GameResultRow)>, anyhow::Error> {
        Err(anyhow::Error::msg("finished_games() unimplemented"))
    }
    async fn pgn(&self, _: RowId) -> Result<String, anyhow::Error> {
        Err(anyhow::Error::msg("pgn() unimplemented"))
    }
    async fn client_performance(&self) -> Result<Vec<ClientPerformanceRecord>, anyhow::Error> {
        Err(anyhow::Error::msg("client_performance() unimplemented"))
    }
}

pub struct SqlxDatabase<DB: sqlx::Database> {
    pub pool: sqlx::Pool<DB>,
}

impl<DB: sqlx::Database> Clone for SqlxDatabase<DB> {
    fn clone(&self) -> Self { Self { pool: self.pool.clone() } }
}

impl SqlxDatabase<sqlx::Sqlite> {
    pub fn new(db_address: &str) -> Result<Self, anyhow::Error> {
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(db_address)
            .create_if_missing(true);
        let pool = async_std::task::block_on(sqlx::SqlitePool::connect_with(options))?;
        Ok(Self { pool })
    }
}

#[allow(dead_code)]
impl SqlxDatabase<sqlx::Postgres> {
    pub fn new(db_address: &str) -> Result<Self, anyhow::Error> {
        let options = sqlx::postgres::PgPoolOptions::new();
        let pool = async_std::task::block_on(options.connect(db_address))?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl<DB> DatabaseReader for SqlxDatabase<DB>
where
    DB: sqlx::Database,
    for<'q> i64: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> String: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> bool: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> OffsetDateTime: sqlx::Type<DB> + sqlx::Encode<'q, DB>,
    for<'q> PrimitiveDateTime: sqlx::Type<DB> + sqlx::Decode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    async fn finished_games(
        &self, game_end_time_range: Range<OffsetDateTime>, only_rated: bool,
    ) -> Result<Vec<(RowId, GameResultRow)>, anyhow::Error> {
        let rows = sqlx::query::<DB>(
            "SELECT
                rowid,
                git_version,
                invocation_id,
                game_start_time,
                game_end_time,
                player_red_a,
                player_red_b,
                player_blue_a,
                player_blue_b,
                result,
                rated
             FROM finished_games
             WHERE
                (game_end_time >= $1 AND game_end_time < $2)
                AND
                ($3 OR (rated AND game_start_time IS NOT NULL))
             ORDER BY game_end_time",
        )
        .bind(game_end_time_range.start)
        .bind(game_end_time_range.end)
        .bind(!only_rated)
        .fetch_all(&self.pool)
        .await?;
        let (oks, errs): (Vec<_>, _) = rows
            .into_iter()
            .map(|row| -> Result<_, anyhow::Error> {
                Ok((
                    RowId {
                        // Turns out rowid is sensitive for sqlx.
                        id: row.try_get("rowid")?,
                    },
                    GameResultRow {
                        git_version: row.try_get("git_version")?,
                        invocation_id: row.try_get("invocation_id")?,
                        // Timestamps need to be re-coded because Postgres
                        // TIMESTAMP datatype can only be decoded as
                        // PrimitiveDateTime, while to get OffsetDateTime,
                        // TIMESTAMPZ needs to be used which is not supported
                        // in MySQL.
                        // Encoding of timestamps doesn't have such issues:
                        // the library converts to UTC and encodes.
                        game_start_time: Option::map(
                            row.try_get("game_start_time")?,
                            PrimitiveDateTime::assume_utc,
                        ),
                        game_end_time: Option::map(
                            row.try_get("game_end_time")?,
                            PrimitiveDateTime::assume_utc,
                        ),
                        player_red_a: row.try_get("player_red_a")?,
                        player_red_b: row.try_get("player_red_b")?,
                        player_blue_a: row.try_get("player_blue_a")?,
                        player_blue_b: row.try_get("player_blue_b")?,
                        result: row.try_get("result")?,
                        game_pgn: String::new(),
                        rated: row.try_get("rated")?,
                    },
                ))
            })
            .partition(Result::is_ok);
        if !errs.is_empty() {
            error!(
                "Failed to parse rows from the DB; sample errors: {:?}",
                errs.iter()
                    .take(5)
                    .map(|x| x.as_ref().err().unwrap().to_string())
                    .collect::<Vec<_>>()
            );
        }
        if oks.is_empty() && !errs.is_empty() {
            // None of the rows parsed, return the first error.
            Err(errs.into_iter().next().unwrap().unwrap_err())
        } else {
            Ok(oks.into_iter().map(Result::unwrap).collect())
        }
    }

    async fn pgn(&self, rowid: RowId) -> Result<String, anyhow::Error> {
        sqlx::query("SELECT game_pgn from finished_games WHERE rowid = $1")
            .bind(rowid.id)
            .fetch_one(&self.pool)
            .await?
            .try_get("game_pgn")
            .map_err(anyhow::Error::from)
    }

    async fn client_performance(&self) -> Result<Vec<ClientPerformanceRecord>, anyhow::Error> {
        let rows = sqlx::query::<DB>(
            "SELECT
                git_version,
                user_agent,
                time_zone,
                ping_p50,
                ping_p90,
                ping_p99,
                ping_n,
                update_state_p50,
                update_state_p90,
                update_state_p99,
                update_state_n
             FROM client_performance",
        )
        .fetch_all(&self.pool)
        .await?;
        let (oks, errs): (Vec<_>, _) = rows
            .into_iter()
            .map(|row| -> Result<_, anyhow::Error> {
                let ping_stats = MeterStats {
                    p50: row.try_get_u64("ping_p50")?,
                    p90: row.try_get_u64("ping_p90")?,
                    p99: row.try_get_u64("ping_p99")?,
                    num_values: row.try_get_u64("ping_n")?,
                };
                let update_state_stats = MeterStats {
                    p50: row.try_get_u64("update_state_p50")?,
                    p90: row.try_get_u64("update_state_p90")?,
                    p99: row.try_get_u64("update_state_p99")?,
                    num_values: row.try_get_u64("update_state_n")?,
                };
                Ok(ClientPerformanceRecord {
                    git_version: row.try_get("git_version")?,
                    user_agent: row.try_get("user_agent")?,
                    time_zone: row.try_get("time_zone")?,
                    ping_stats,
                    update_state_stats,
                })
            })
            .partition(Result::is_ok);
        if !errs.is_empty() {
            error!(
                "Failed to parse rows from the DB; sample errors: {:?}",
                errs.iter()
                    .take(5)
                    .map(|x| x.as_ref().err().unwrap().to_string())
                    .collect::<Vec<_>>()
            );
        }
        if oks.is_empty() && !errs.is_empty() {
            // None of the rows parsed, return the first error.
            Err(errs.into_iter().next().unwrap().unwrap_err())
        } else {
            Ok(oks.into_iter().map(Result::unwrap).collect())
        }
    }
}

pub trait HasRowidColumnDefinition {
    const ROWID_COLUMN_DEFINITION: &'static str;
}

impl HasRowidColumnDefinition for sqlx::Sqlite {
    const ROWID_COLUMN_DEFINITION: &'static str = "";
}

impl HasRowidColumnDefinition for sqlx::Postgres {
    const ROWID_COLUMN_DEFINITION: &'static str = "rowid BIGSERIAL PRIMARY KEY,";
}

#[async_trait]
impl<DB> DatabaseWriter for SqlxDatabase<DB>
where
    DB: sqlx::Database + HasRowidColumnDefinition,
    String: Type<DB> + for<'q> Encode<'q, DB>,
    i64: Type<DB> + for<'q> Encode<'q, DB>,
    Option<i64>: Type<DB> + for<'q> Encode<'q, DB>,
    Option<OffsetDateTime>: Type<DB> + for<'q> Encode<'q, DB>,
    bool: Type<DB> + for<'q> Encode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
{
    async fn create_tables(&self) -> anyhow::Result<()> {
        // TODO: Include match_id in finished_games.
        let rowid_column_definition = DB::ROWID_COLUMN_DEFINITION;
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
            )
            .as_str(),
        )
        .execute(&self.pool)
        .await?;
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    async fn add_finished_game(&self, row: GameResultRow) -> anyhow::Result<()> {
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    // TODO: Save time when performance was recorded.
    async fn add_client_performance(
        &self, perf: &BughouseClientPerformance, invocation_id: &str,
    ) -> anyhow::Result<()> {
        let stats = &perf.stats;
        let ping = stats.get("ping");
        let turn_confirmation = stats.get("turn_confirmation");
        let process_outgoing_events = stats.get("process_outgoing_events");
        let process_notable_events = stats.get("process_notable_events");
        let refresh = stats.get("refresh");
        let update_state = stats.get("update_state");
        let update_clock = stats.get("update_clock");
        let update_drag_state = stats.get("update_drag_state");
        sqlx::query(
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
                .bind(invocation_id.to_owned())
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
            .execute(&self.pool).await?;
        Ok(())
    }
}
