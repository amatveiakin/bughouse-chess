// TODO: streaming support + APIs.
use std::ops::Range;

use log::error;
use sqlx::prelude::*;
use tide::utils::async_trait;
use time::{OffsetDateTime, PrimitiveDateTime};

use bughouse_chess::persistence::*;

#[derive(Copy, Clone, Debug)]
pub struct RowId {
    pub id: i64,
}

#[async_trait]
pub trait DatabaseReader {
    async fn finished_games(
        &self,
        game_end_time_range: Range<OffsetDateTime>,
        only_rated: bool,
    ) -> Result<Vec<(RowId, GameResultRow)>, anyhow::Error>;
    async fn pgn(&self, rowid: RowId) -> Result<String, anyhow::Error>;
}

pub struct UnimplementedDatabase {}

#[async_trait]
impl DatabaseReader for UnimplementedDatabase {
    async fn finished_games( &self, _: Range<OffsetDateTime>, _: bool) -> Result<Vec<(RowId, GameResultRow)>, anyhow::Error> {
        Err(anyhow::Error::msg("finished_games() unimplemented"))
    }
    async fn pgn(&self, _: RowId) -> Result<String, anyhow::Error> {
        Err(anyhow::Error::msg("pgn() unimplemented"))
    }
}

pub struct SqlxDatabase<DB: sqlx::Database> {
    pool: sqlx::Pool<DB>,
}

impl<DB: sqlx::Database> Clone for SqlxDatabase<DB> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

impl SqlxDatabase<sqlx::Sqlite> {
    pub fn new(db_address: &str) -> Result<Self, anyhow::Error> {
        let options = sqlx::sqlite::SqlitePoolOptions::new();
        let pool = async_std::task::block_on(options.connect(&format!("{db_address}")))?;
        Ok(Self { pool })
    }
}

#[allow(dead_code)]
impl SqlxDatabase<sqlx::Postgres> {
    pub fn new(db_address: &str) -> Result<Self, anyhow::Error> {
        let options = sqlx::postgres::PgPoolOptions::new();
        let pool = async_std::task::block_on(options.connect(&format!("{db_address}")))?;
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
        &self,
        game_end_time_range: Range<OffsetDateTime>,
        only_rated: bool,
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
            Err(errs.into_iter().next().unwrap().unwrap_err().into())
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
}
