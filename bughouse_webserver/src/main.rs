// TODO: streaming support + APIs.
use std::collections::HashMap;
use std::ops::Range;

use clap::Parser;
use log::error;
use sqlx::prelude::*;
use tide::http::Mime;
use tide::{Request, Response, StatusCode};
use tide_jsx::*;
use time::OffsetDateTime;

use bughouse_chess::persistence::*;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "0.0.0.0:14362")]
    bind_address: String,

    #[arg(long)]
    sqlite_db: Option<String>,

    #[arg(long)]
    postgres_db: Option<String>,
}

#[async_std::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    match (args.sqlite_db, args.postgres_db) {
        (None, None) => return Err(anyhow::Error::msg("Database address was not specified.")),
        (Some(_), Some(_)) => {
            return Err(anyhow::Error::msg(
                "Both sqlite-db and postgres-db were specified.",
            ))
        }
        (Some(db), _) => {
            let mut app = tide::with_state(SqlxApp::<sqlx::Sqlite>::new(&db)?);
            SqlxApp::<sqlx::Sqlite>::register_handlers(&mut app);
            app.listen(args.bind_address).await?;
        }
        (_, Some(_db)) => {
            // TODO: SQL needs to be adjusted. rowid column does not exist in postgresql.
            return Err(anyhow::Error::msg(
                "Postgresql reader is not implemented yet.",
            ));
            // let mut app = tide::with_state(SqlxApp::<sqlx::Postgres>::new(&db)?);
            // SqlxApp::<sqlx::Postgres>::register_handlers(&mut app);
            // app.listen(args.bind_address).await?;
        }
    }
    Ok(())
}

#[derive(Copy, Clone, Debug)]
struct RowId {
    id: i64,
}

struct SqlxApp<DB: sqlx::Database> {
    pool: sqlx::Pool<DB>,
}

impl<DB: sqlx::Database> Clone for SqlxApp<DB> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

impl SqlxApp<sqlx::Sqlite> {
    pub fn new(db_address: &str) -> Result<Self, anyhow::Error> {
        let options = sqlx::sqlite::SqlitePoolOptions::new();
        let pool = async_std::task::block_on(options.connect(&format!("{db_address}")))?;
        Ok(Self { pool })
    }
}

#[allow(dead_code)]
impl SqlxApp<sqlx::Postgres> {
    pub fn new(db_address: &str) -> Result<Self, anyhow::Error> {
        let options = sqlx::postgres::PgPoolOptions::new();
        let pool = async_std::task::block_on(options.connect(&format!("{db_address}")))?;
        Ok(Self { pool })
    }
}

impl<DB> SqlxApp<DB>
where
    DB: sqlx::Database,
    for<'q> i64: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> String: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> OffsetDateTime: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    pub async fn finished_games(
        &self,
        game_start_time_range: Range<OffsetDateTime>,
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
                result
             FROM finished_games
             WHERE game_start_time >= ?1 AND game_start_time < ?2
             ORDER BY game_start_time DESC",
        )
        .bind(game_start_time_range.start)
        .bind(game_start_time_range.end)
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
                        game_start_time: row.try_get("game_start_time")?,
                        game_end_time: row.try_get("game_end_time")?,
                        player_red_a: row.try_get("player_red_a")?,
                        player_red_b: row.try_get("player_red_b")?,
                        player_blue_a: row.try_get("player_blue_a")?,
                        player_blue_b: row.try_get("player_blue_b")?,
                        result: row.try_get("result")?,
                        game_pgn: String::new(),
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

    pub async fn pgn(&self, rowid: RowId) -> Result<String, anyhow::Error> {
        sqlx::query("SELECT game_pgn from finished_games WHERE ROWID = ?")
            .bind(rowid.id)
            .fetch_one(&self.pool)
            .await?
            .try_get("game_pgn")
            .map_err(anyhow::Error::from)
    }

    const STYLESHEET: &str = "
table, th, td {
    border: 1px solid black;
    border-collapse: collapse;
}
td, th {
    padding-left: 10px;
    padding-right: 10px;
}
";

    fn register_handlers(app: &mut tide::Server<Self>) {
        app.at("/dyn/games").get(Self::handle_games);
        app.at("/dyn/pgn/:rowid").get(SqlxApp::hanle_pgn);
        app.at("/dyn/stats").get(|r| Self::handle_stats(r, None));
        app.at("/dyn/stats/:duration")
            .get(Self::handle_stats_with_duration);

        app.with(tide::log::LogMiddleware::new());

        app.with(tide::utils::After(|mut res: Response| async {
            if let Some(err) = res.error() {
                let msg = format!("Error: {:?}", err);
                res.set_status(err.status());
                res.set_body(msg);
            }
            Ok(res)
        }));
    }

    async fn handle_games(req: Request<Self>) -> tide::Result {
        let games = req
            .state()
            .finished_games(OffsetDateTime::UNIX_EPOCH..OffsetDateTime::now_utc())
            .await
            .map_err(anyhow::Error::from)?;
        let table_body = games
            .iter()
            .map(|(rowid, game)| {
                let (start_date, start_time) = format_timestamp_date_and_time(game.game_start_time)
                    .unwrap_or(("-".into(), "-".into()));
                let red_team = format!("{}, {}", game.player_red_a, game.player_red_b);
                let blue_team = format!("{}, {}", game.player_blue_a, game.player_blue_b);
                let (winners, losers, drawers) = match game.result.as_str() {
                    "DRAW" => (
                        "".to_string(),
                        "".to_string(),
                        format!("{}, {}", red_team, blue_team),
                    ),
                    "VICTORY_RED" => (red_team, blue_team, "".to_string()),
                    "VICTORY_BLUE" => (blue_team, red_team, "".to_string()),
                    _ => ("".to_string(), "".to_string(), "".to_string()),
                };
                rsx! {<tr>
                    <td>{start_date}</td>
                    <td>{start_time}</td>
                    <td>{winners}</td>
                    <td>{losers}</td>
                    <td>{drawers}</td>
                    <td><a href={format!("/dyn/pgn/{}", rowid.id)}>{"pgn💾"}</a></td>
                </tr>}
            })
            .collect::<Vec<_>>();

        let h: String = html! {
            <html>
                <style>
                    {Self::STYLESHEET}
                </style>
            <head>
            </head>
            <body>
              <table>
                <tr>
                    <th>{"Date"}</th>
                    <th>{"Time (UTC)"}</th>
                    <th>{"Winners"}</th>
                    <th>{"Losers"}</th>
                    <th>{"Drawers"}</th>
                    <th>{"Pgn"}</th>
                </tr>
                {table_body}
              </table>
            </body>
            </html>
        };
        let mut resp = Response::new(StatusCode::Ok);
        resp.set_content_type(Mime::from("text/html; charset=UTF-8"));
        resp.set_body(h);
        Ok(resp)
    }

    async fn hanle_pgn(req: Request<Self>) -> tide::Result {
        let rowid = req.param("rowid")?.parse()?;
        let p = req.state().pgn(RowId { id: rowid }).await?;
        let mut resp = Response::new(StatusCode::Ok);
        resp.insert_header(
            "Content-Disposition",
            format!("attachment; filename=\"game{rowid}.pgn\""),
        );
        resp.set_body(p);
        Ok(resp)
    }

    async fn handle_stats_with_duration(req: Request<Self>) -> tide::Result {
        let duration_str = req.param("duration")?;
        let duration = humantime::parse_duration(duration_str)?;
        Self::handle_stats(req, Some(duration.try_into()?)).await
    }

    async fn handle_stats(req: Request<Self>, lookback: Option<time::Duration>) -> tide::Result {
        let now = OffsetDateTime::now_utc();
        let range_start = match lookback {
            None => OffsetDateTime::UNIX_EPOCH,
            Some(d) => now.saturating_sub(d),
        };
        let games = req
            .state()
            .finished_games(range_start..now)
            .await
            .map_err(anyhow::Error::from)?;
        let mut player_stats = HashMap::<String, RawStats>::new();
        let mut team_stats = HashMap::<[String; 2], RawStats>::new();

        for (_, game) in games.into_iter() {
            let red_team = sort([game.player_red_a, game.player_red_b]);
            let blue_team = sort([game.player_blue_a, game.player_blue_b]);
            match game.result.as_str() {
                "DRAW" => {
                    for p in red_team.iter().chain(blue_team.iter()).cloned() {
                        player_stats.entry(p).or_default().draws += 1;
                    }
                    for team in [red_team, blue_team].iter() {
                        team_stats.entry(team.clone()).or_default().draws += 1;
                    }
                }
                "VICTORY_RED" => {
                    for p in red_team.iter().cloned() {
                        player_stats.entry(p).or_default().wins += 1;
                    }
                    for p in blue_team.iter().cloned() {
                        player_stats.entry(p).or_default().losses += 1;
                    }
                    team_stats.entry(red_team).or_default().wins += 1;
                    team_stats.entry(blue_team).or_default().losses += 1;
                }
                "VICTORY_BLUE" => {
                    for p in red_team.iter().cloned() {
                        player_stats.entry(p).or_default().losses += 1;
                    }
                    for p in blue_team.iter().cloned() {
                        player_stats.entry(p).or_default().wins += 1;
                    }
                    team_stats.entry(red_team).or_default().losses += 1;
                    team_stats.entry(blue_team).or_default().wins += 1;
                }
                _ => {}
            }
        }

        let mut final_player_stats = process_stats(player_stats.into_iter());
        let mut final_team_stats = process_stats(
            team_stats
                .into_iter()
                .map(|([t0, t1], s)| (format!("{}, {}", t0, t1), s)),
        );
        final_player_stats.sort_unstable_by(|a, b| b.pointrate.total_cmp(&a.pointrate));
        final_team_stats.sort_unstable_by(|a, b| b.pointrate.total_cmp(&a.pointrate));

        let leaderboard = |final_stats: Vec<FinalStats>| {
            final_stats
                .into_iter()
                .map(|s| {
                    rsx! {
                        <tr>
                            <td>{s.name}</td>
                            <td>{format!("{:.3}", s.pointrate)}</td>
                            <td>{s.points}</td>
                            <td>{s.games}</td>
                            <td>{s.wins}</td>
                            <td>{s.losses}</td>
                            <td>{s.draws}</td>
                        </tr>
                    }
                })
                .collect::<Vec<_>>()
        };

        let h: String = html! {
            <html>
                <style>
                    {Self::STYLESHEET}
                </style>
            <head>
            </head>
            <body>
              <table>
                <p>{"Player Leaderboard"}</p>
                <tr>
                    <th>{"Player"}</th>
                    <th>{"Pointrate"}</th>
                    <th>{"Points"}</th>
                    <th>{"Games"}</th>
                    <th>{"Wins"}</th>
                    <th>{"Losses"}</th>
                    <th>{"Draws"}</th>
                </tr>
                {leaderboard(final_player_stats)}
              </table>
              <table>
                <p>{"Team Leaderboard"}</p>
                <tr>
                    <th>{"Team"}</th>
                    <th>{"Pointrate"}</th>
                    <th>{"Points"}</th>
                    <th>{"Games"}</th>
                    <th>{"Wins"}</th>
                    <th>{"Losses"}</th>
                    <th>{"Draws"}</th>
                </tr>
                {leaderboard(final_team_stats)}
              </table>
            </body>
            </html>
        };
        let mut resp = Response::new(StatusCode::Ok);
        resp.set_content_type(Mime::from("text/html; charset=UTF-8"));
        resp.set_body(h);
        Ok(resp)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RawStats {
    draws: usize,
    wins: usize,
    losses: usize,
}

#[derive(Debug, Clone, Default)]
struct FinalStats {
    name: String,
    games: usize,
    draws: usize,
    wins: usize,
    losses: usize,
    points: f64,
    pointrate: f64,
}

fn process_stats<I: Iterator<Item = (String, RawStats)>>(raw_stats: I) -> Vec<FinalStats> {
    raw_stats
        .map(|(name, s)| {
            let games = s.draws + s.wins + s.losses;
            let points = s.wins as f64 + 0.5 * s.draws as f64;
            FinalStats {
                name,
                games,
                points,
                draws: s.draws,
                wins: s.wins,
                losses: s.losses,
                pointrate: points / games as f64,
            }
        })
        .collect()
}

fn format_timestamp_date_and_time(maybe_ts: Option<OffsetDateTime>) -> Option<(String, String)> {
    let datetime = maybe_ts?;
    let date = datetime
        .format(&time::macros::format_description!(
            "[year]-[month]-[day], [weekday repr:short]"
        ))
        .ok()?;
    let time = datetime
        .format(&time::macros::format_description!(
            "[hour]:[minute]:[second]"
        ))
        .ok()?;
    Some((date, time))
}

fn sort<A, T>(mut array: A) -> A
where
    A: AsMut<[T]>,
    T: Ord,
{
    array.as_mut().sort();
    array
}
