use bughouse_chess::my_git_version;
use bughouse_chess::server_hooks::ServerHooks;
use log::error;
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

impl<DB: DatabaseWriter> ServerHooks for DatabaseServerHooks<DB> {
    fn on_client_performance_report(&mut self, perf: &BughouseClientPerformance) {
        if let Err(e) = async_std::task::block_on(
            self.db.add_client_performance(perf, self.invocation_id.as_str()),
        ) {
            error!("Error persisting client performance: {}", e);
        }
    }
    fn on_game_over(
        &mut self, game: &BughouseGame, game_start_offset_time: Option<time::OffsetDateTime>,
        round: usize,
    ) {
        let Some(row) = self.game_result(game, game_start_offset_time, round) else {
            error!("Error extracting game result from:\n{:#?}", game);
            return;
        };
        if let Err(e) = async_std::task::block_on(self.db.add_finished_game(row)) {
            error!("Error persisting game result: {}", e);
        }
    }
}

impl<DB: DatabaseWriter> DatabaseServerHooks<DB> {
    fn game_result(
        &self, game: &BughouseGame, game_start_offset_time: Option<time::OffsetDateTime>,
        round: usize,
    ) -> Option<GameResultRow> {
        let result = game_result_str(game.status())?;
        let get_player = |team, board_idx| {
            game.board(board_idx)
                .player_name(get_bughouse_force(team, board_idx))
                .to_owned()
        };
        Some(GameResultRow {
            git_version: my_git_version!().to_owned(),
            invocation_id: self.invocation_id.to_string(),
            game_start_time: game_start_offset_time,
            game_end_time: Some(OffsetDateTime::now_utc()),
            player_red_a: get_player(Team::Red, BughouseBoard::A),
            player_red_b: get_player(Team::Red, BughouseBoard::B),
            player_blue_a: get_player(Team::Blue, BughouseBoard::A),
            player_blue_b: get_player(Team::Blue, BughouseBoard::B),
            result,
            game_pgn: pgn::export_to_bpgn(pgn::BpgnExportFormat::default(), game, round),
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
