// TODO: streaming support + APIs.
use tide::http::Mime;
use tide::{Request, Response, StatusCode};
use tide_jsx::*;
use time::OffsetDateTime;

use crate::database::{self, DatabaseReader};
use crate::game_stats::{GroupStats, RawStats};
use crate::history_graphs;

pub trait SuitableServerState: Sync + Send + Clone + 'static {
    type DB: Sync + Send + database::DatabaseReader;
    fn db(&self) -> &Self::DB;
    fn static_content_url_prefix(&self) -> &str;
}

// Purely type-level construct to avoid making every handler function generic.
// They are all generic by the virtue of being inside the generic impl.
pub struct Handlers<ST> {
    _phantom: ST,
}

impl<ST: SuitableServerState> Handlers<ST> {
    pub fn register_handlers(app: &mut tide::Server<ST>) {
        app.at("/dyn/games").get(Self::handle_games);
        app.at("/dyn/pgn/:rowid").get(Self::hanle_pgn);
        app.at("/dyn/stats").get(|r| Self::handle_stats(r, None));
        app.at("/dyn/stats/:duration")
            .get(Self::handle_stats_with_duration);
        app.at("/dyn/history")
            .get(|req| Self::handle_history(req, history_graphs::XAxis::Timestamp));
        app.at("/dyn/history/pergame")
            .get(|req| Self::handle_history(req, history_graphs::XAxis::UpdateIndex));

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

    async fn handle_games(req: Request<ST>) -> tide::Result {
        let games = database::DatabaseReader::finished_games(
            req.state().db(),
            OffsetDateTime::UNIX_EPOCH..OffsetDateTime::now_utc(),
            /*only_rated=*/ false,
        )
        .await
        .map_err(anyhow::Error::from)?;
        let table_body = games
            .iter()
            .rev()
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
                let rated_str = if game.rated { "✔️" } else { "🛇" };
                rsx! {<tr>
                    <td>{start_date}</td>
                    <td class={"centered"}>{start_time}</td>
                    <td class={"centered"}>{rated_str}</td>
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
                    {raw!(Self::STYLESHEET)}
                </style>
            <head>
            </head>
            <body>
              <table>
                <tr>
                    <th>{"Date"}</th>
                    <th>{"Time (UTC)"}</th>
                    <th>{"Rated"}</th>
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

    async fn hanle_pgn(req: Request<ST>) -> tide::Result {
        let rowid = req.param("rowid")?.parse()?;
        let p = req.state().db().pgn(database::RowId { id: rowid }).await?;
        let mut resp = Response::new(StatusCode::Ok);
        resp.insert_header(
            "Content-Disposition",
            format!("attachment; filename=\"game{rowid}.pgn\""),
        );
        resp.set_body(p);
        Ok(resp)
    }

    async fn handle_stats_with_duration(req: Request<ST>) -> tide::Result {
        let duration_str = req.param("duration")?;
        let duration = humantime::parse_duration(duration_str)?;
        Self::handle_stats(req, Some(duration.try_into()?)).await
    }

    async fn handle_stats(req: Request<ST>, lookback: Option<time::Duration>) -> tide::Result {
        let now = OffsetDateTime::now_utc();
        let range_start = match lookback {
            None => OffsetDateTime::UNIX_EPOCH,
            Some(d) => now.saturating_sub(d),
        };
        let games = req
            .state()
            .db()
            .finished_games(range_start..now, /*only_rated=*/ true)
            .await
            .map_err(anyhow::Error::from)?;

        // TODO: initialize from a persisted state and only look at games played since the last
        // committed game.
        let mut all_stats = GroupStats::default();

        for (_, game) in games.into_iter() {
            all_stats.update(&game)?;
        }

        let mut final_player_stats = process_stats(all_stats.per_player.into_iter());
        let mut final_team_stats = process_stats(
            all_stats
                .per_team
                .into_iter()
                .map(|([t0, t1], s)| (format!("{}, {}", t0, t1), s)),
        );
        final_player_stats.sort_unstable_by(|a, b| b.pointrate.total_cmp(&a.pointrate));
        final_team_stats.sort_unstable_by(|a, b| b.elo.partial_cmp(&a.elo).unwrap());

        let team_leaderboard = |final_stats: Vec<FinalStats>| {
            final_stats
                .into_iter()
                .map(|s| {
                    rsx! {
                        <tr>
                            <td>{s.name}</td>
                            <td>{format!("{:.3}", s.pointrate)}</td>
                            <td>{s.elo.map_or("".to_owned(), |e| format!("{:.3}", e))}</td>
                            <td>{s.rating.map_or("".to_owned(), |r| format!("{:.3}", r))}</td>
                            <td>{s.rating_uncertainty.map_or("".to_owned(), |r| format!("{:.3}", r))}</td>
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
        let player_leaderboard = |final_stats: Vec<FinalStats>| {
            final_stats
                .into_iter()
                .map(|s| {
                    rsx! {
                        <tr>
                            <td>{s.name}</td>
                            <td>{format!("{:.3}", s.pointrate)}</td>
                            <td>{s.rating.map_or("".to_owned(), |r| format!("{:.3}", r))}</td>
                            <td>{s.rating_uncertainty.map_or("".to_owned(), |r| format!("{:.3}", r))}</td>
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
                <script type={"text/javascript"}
                        src={req.state().static_content_url_prefix().to_owned() + "/jstable.js"}>
                    {";"}
                </script>
                <script>
                    {raw!(r##"
                        function initJSTables() {
                            new JSTable("#player-leaderboard-table", {
                                'perPageSelect': [5, 10, 20, 100, 1000],
                                'perPage': 10,
                                'columns': [{
                                    select: 1,
                                    sortable: true,
                                    sort: 'desc'
                                }]
                            });
                            new JSTable("#team-leaderboard-table", {
                                'perPageSelect': [5, 10, 20, 100, 1000],
                                'perPage': 10,
                                'columns': [{
                                    select: 2,
                                    sortable: true,
                                    sort: 'desc'
                                }]
                            });
                        }
                    "##)}
                </script>
                <style>
                    {raw!(Self::STYLESHEET)}
                </style>
            <head>
            </head>
            <body onload={"initJSTables()"}>
              <h2>{"Player Leaderboard"}</h2>
              <table id={"player-leaderboard-table"}>
                <thead>
                    <tr>
                        <th>{"Player"}</th>
                        <th>{"Pointrate"}</th>
                        <th><a href={"https://docs.rs/skillratings/latest/skillratings/weng_lin/fn.weng_lin_two_teams.html"}>{"Rating"}</a></th>
                        <th>{"σ"}</th>
                        <th>{"Points"}</th>
                        <th>{"Games"}</th>
                        <th>{"Wins"}</th>
                        <th>{"Losses"}</th>
                        <th>{"Draws"}</th>
                    </tr>
                </thead>
                <tbody>
                    {player_leaderboard(final_player_stats)}
                </tbody>
              </table>
              <h2>{"Team Leaderboard"}</h2>
              <table id={"team-leaderboard-table"}>
                <thead>
                    <tr>
                        <th>{"Team"}</th>
                        <th>{"Pointrate"}</th>
                        <th>{"Elo"}</th>
                        <th><a href={"https://docs.rs/skillratings/latest/skillratings/weng_lin/fn.weng_lin.html"}>{"Rating"}</a></th>
                        <th>{"σ"}</th>
                        <th>{"Points"}</th>
                        <th>{"Games"}</th>
                        <th>{"Wins"}</th>
                        <th>{"Losses"}</th>
                        <th>{"Draws"}</th>
                    </tr>
                </thead>
                <tbody>
                    {team_leaderboard(final_team_stats)}
                </tbody>
              </table>
            </body>
            </html>
        };
        let mut resp = Response::new(StatusCode::Ok);
        resp.set_content_type(Mime::from("text/html; charset=UTF-8"));
        resp.set_body(h);
        Ok(resp)
    }

    async fn handle_history(req: Request<ST>, x_axis: history_graphs::XAxis) -> tide::Result {
        let now = OffsetDateTime::now_utc();
        let games = req
            .state()
            .db()
            .finished_games(OffsetDateTime::UNIX_EPOCH..now, /*only_rated=*/ true)
            .await
            .map_err(anyhow::Error::from)?;

        // TODO: initialize from a persisted state and only look at games played since the last
        // committed game.
        let mut all_stats = GroupStats::<Vec<RawStats>>::default();

        for (_, game) in games.into_iter() {
            all_stats.update(&game)?;
        }

        let players_history_graph_html =
            history_graphs::players_rating_graph_html(&all_stats, x_axis);
        let teams_history_graph_html = history_graphs::teams_elo_graph_html(&all_stats, x_axis);
        let h: String = html! {
            <html>
            <head>
                {raw!(r#"<script src="https://cdn.plot.ly/plotly-2.16.1.min.js"></script>"#)}
            </head>
            <body>
                {raw!(players_history_graph_html.as_str())}
                {raw!(teams_history_graph_html.as_str())}
            </body>
            </html>
        };
        let mut resp = Response::new(StatusCode::Ok);
        resp.set_content_type(Mime::from("text/html; charset=UTF-8"));
        resp.set_body(h);
        Ok(resp)
    }

    // TODO: move to static content as well.
    const STYLESHEET: &str = r#"
table, th, td {
    border: 1px solid black;
    border-collapse: collapse;
}
td, th {
    padding-left: 10px;
    padding-right: 10px;
}
td.centered {
    text-align: center;
}

.dt-sorter:not(.asc, .desc)::after {
    filter: grayscale(50%);
    content: "⬍";
    /* Black arrow converted to grey arrow. */
    filter: invert() brightness(0.5);
}
.dt-sorter.asc::after {
    content: "⬆";
}
.dt-sorter.desc::after {
    content: "⬇";
}

/* Copied from jstable */
.hidden {
    display: none;
}
 .dt-pagination ul {
	 margin: 0;
	 padding-left: 0;
}
 .dt-pagination ul li {
	 list-style: none;
	 float: left;
}
 .dt-pagination a, .dt-pagination span {
	 border: 1px solid transparent;
	 float: left;
	 margin-left: 2px;
	 padding: 6px 12px;
	 position: relative;
	 text-decoration: none;
	 color: inherit;
}
 .dt-pagination a:hover {
	 background-color: #d9d9d9;
}
 .dt-pagination .active a, .dt-pagination .active a:focus, .dt-pagination .active a:hover {
	 background-color: #d9d9d9;
	 cursor: default;
}
 .dt-pagination .dt-ellipsis span {
	 cursor: not-allowed;
}
 .dt-pagination .disabled a, .dt-pagination .disabled a:focus, .dt-pagination .disabled a:hover {
	 cursor: not-allowed;
	 opacity: 0.4;
}
 .dt-pagination .pager a {
	 font-weight: bold;
}
"#;
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
    elo: Option<f64>,
    rating: Option<f64>,
    rating_uncertainty: Option<f64>,
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
                elo: s.elo.map(|e| e.rating),
                rating: s.rating.map(|r| r.rating),
                rating_uncertainty: s.rating.map(|r| r.uncertainty),
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
