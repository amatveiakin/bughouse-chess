use bughouse_chess::my_git_version;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::utc_time::UtcDateTime;
use enum_map::enum_map;
use itertools::Itertools;
use log::error;
use strum::IntoEnumIterator;
use time::OffsetDateTime;

use crate::bughouse_prelude::*;
use crate::persistence::*;

pub struct DatabaseServerHooks<DB> {
    invocation_id: String,
    db: DB,
}

impl<DB: DatabaseWriter> DatabaseServerHooks<DB> {
    pub fn new(db: DB) -> anyhow::Result<Self> {
        async_std::task::block_on(db.create_tables())?;
        Ok(Self {
            invocation_id: uuid::Uuid::new_v4().to_string(),
            db,
        })
    }
}

impl<DB: DatabaseReader + DatabaseWriter> ServerHooks for DatabaseServerHooks<DB> {
    fn on_client_performance_report(&mut self, perf: &BughouseClientPerformance) {
        if let Err(e) = async_std::task::block_on(
            self.db.add_client_performance(perf, self.invocation_id.as_str()),
        ) {
            error!("Error persisting client performance: {}", e);
        }
    }

    fn on_game_over(
        &mut self, game: &BughouseGame, game_start_time: UtcDateTime, game_end_time: UtcDateTime,
        round: u64,
    ) {
        let Some(row) = self.game_result(game, game_start_time, game_end_time, round) else {
            error!("Error extracting game result from:\n{:#?}", game);
            return;
        };
        if let Err(e) = async_std::task::block_on(self.db.add_finished_game(row)) {
            error!("Error persisting game result: {}", e);
        }
    }

    fn get_games_by_user(&self, user_name: &str) -> Result<Vec<FinishedGameDescription>, String> {
        let full_time_range = OffsetDateTime::UNIX_EPOCH..OffsetDateTime::now_utc();
        // TODO: Optimized SQL query to fetch only games by a given player.
        let rows = async_std::task::block_on(self.db.finished_games(full_time_range, false))
            .map_err(|err| format!("Error reading game history: {err:?}"))?;
        let games = rows
            .into_iter()
            .filter_map(|(rowid, row)| {
                let game_id = rowid.id;
                let game_start_time = row.game_start_time?.into();
                let mut team_players = enum_map! { _ => vec![] };
                team_players[Team::Red].push((BughouseBoard::A, row.player_red_a));
                team_players[Team::Red].push((BughouseBoard::B, row.player_red_b));
                team_players[Team::Blue].push((BughouseBoard::A, row.player_blue_a));
                team_players[Team::Blue].push((BughouseBoard::B, row.player_blue_b));
                for team in Team::iter() {
                    team_players[team]
                        .sort_by_key(|&(board_idx, _)| get_bughouse_force(team, board_idx));
                }
                let mut team_players = team_players
                    .map(|_, players| players.into_iter().map(|(_, name)| name).collect_vec());
                for team in Team::iter() {
                    team_players[team].dedup();
                }
                let user_team = team_players
                    .iter()
                    .find(|(_, players)| players.iter().any(|p| p == user_name))
                    .map(|(team, _)| team);
                let Some(user_team) = user_team else {
                    return None;
                };
                let teammates = std::mem::take(&mut team_players[user_team]);
                let opponents = std::mem::take(&mut team_players[user_team.opponent()]);
                // TODO: Log game result parsing errors.
                let winner = game_result_str_to_winner(&row.result).ok()?;
                let result = match winner {
                    Some(team) => {
                        if team == user_team {
                            SubjectiveGameResult::Victory
                        } else {
                            SubjectiveGameResult::Defeat
                        }
                    }
                    None => SubjectiveGameResult::Draw,
                };
                let rated = row.rated;
                Some(FinishedGameDescription {
                    game_id,
                    game_start_time,
                    teammates,
                    opponents,
                    result,
                    rated,
                })
            })
            .collect();
        Ok(games)
    }

    fn get_game_bpgn(&self, game_id: i64) -> Result<String, String> {
        async_std::task::block_on(self.db.pgn(RowId { id: game_id }))
            .map_err(|err| format!("Error fetching game BPGN: {err:?}"))
    }
}

impl<DB: DatabaseWriter> DatabaseServerHooks<DB> {
    fn game_result(
        &self, game: &BughouseGame, game_start_time: UtcDateTime, game_end_time: UtcDateTime,
        round: u64,
    ) -> Option<GameResultRow> {
        let result = game_result_str(game.status())?;
        let get_player = |team, board_idx| {
            game.board(board_idx)
                .player_name(get_bughouse_force(team, board_idx))
                .to_owned()
        };
        let game_pgn =
            pgn::export_to_bpgn(pgn::BpgnExportFormat::default(), game, game_start_time, round);
        Some(GameResultRow {
            git_version: my_git_version!().to_owned(),
            invocation_id: self.invocation_id.to_string(),
            game_start_time: Some(game_start_time.into()),
            game_end_time: Some(game_end_time.into()),
            player_red_a: get_player(Team::Red, BughouseBoard::A),
            player_red_b: get_player(Team::Red, BughouseBoard::B),
            player_blue_a: get_player(Team::Blue, BughouseBoard::A),
            player_blue_b: get_player(Team::Blue, BughouseBoard::B),
            result,
            game_pgn,
            rated: game.match_rules().rated,
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

fn game_result_str_to_winner(result: &str) -> Result<Option<Team>, String> {
    match result {
        "VICTORY_RED" => Ok(Some(Team::Red)),
        "VICTORY_BLUE" => Ok(Some(Team::Blue)),
        "DRAW" => Ok(None),
        _ => Err(format!("Invalid result string: {result}")),
    }
}
