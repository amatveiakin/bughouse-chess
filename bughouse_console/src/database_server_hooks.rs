use bughouse_chess::server::*;
use bughouse_chess::server_hooks::ServerHooks;
use bughouse_chess::*;
use log::error;
use time::OffsetDateTime;

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
    fn on_client_event(&mut self, event: &BughouseClientEvent) {
        if let BughouseClientEvent::ReportPerformace(performance) = event {
            if let Err(e) = async_std::task::block_on(
                self.db.add_client_performance(performance, self.invocation_id.as_str()),
            ) {
                error!("Error persisting client performance: {}", e);
            }
        }
    }
    fn on_server_broadcast_event(
        &mut self, event: &BughouseServerEvent, maybe_game: Option<&GameState>, round: usize,
    ) {
        let Some(row) = self.game_result(event, maybe_game, round) else {
            return;
        };
        if let Err(e) = async_std::task::block_on(self.db.add_finished_game(row)) {
            error!("Error persisting game result: {}", e);
        }
    }
}

impl<DB: DatabaseWriter> DatabaseServerHooks<DB> {
    fn game_result(
        &self, event: &BughouseServerEvent, maybe_game: Option<&GameState>, round: usize,
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
            game_end_time: Some(OffsetDateTime::now_utc()),
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
        game.game()
            .board(board_idx)
            .player_name(get_bughouse_force(team, board_idx))
            .to_owned()
    };
    (
        get_player(Team::Red, BughouseBoard::A),
        get_player(Team::Red, BughouseBoard::B),
        get_player(Team::Blue, BughouseBoard::A),
        get_player(Team::Blue, BughouseBoard::B),
    )
}
