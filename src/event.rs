use serde::{Serialize, Deserialize};

use crate::board::TurnInput;
use crate::clock::GameInstant;
use crate::game::{TurnRecord, BughouseGameStatus, BughouseBoard};
use crate::grid::Grid;
use crate::pgn::BughouseExportFormat;
use crate::player::{PlayerInGame, Player, Team};
use crate::rules::{Teaming, ChessRules, BughouseRules};
use crate::scores::Scores;


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseServerEvent {
    Error {
        message: String,
    },
    ContestStarted {
        // TODO: Consider moving chess_rules and bughouse_rules here
        teaming: Teaming,
    },
    LobbyUpdated {
        players: Vec<Player>,
    },
    GameStarted {  // TODO: Rename to take reconnection into account
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        starting_grid: Grid,
        players: Vec<(PlayerInGame, BughouseBoard)>,
        time: GameInstant,                // for re-connection
        turn_log: Vec<TurnRecord>,        // for re-connection
        game_status: BughouseGameStatus,  // for re-connection
        scores: Scores,
        // TODO: Send your pending pre-turn, if any
    },
    // Improvement potential: unite `TurnsMade` and `GameOver` into a single event "something happened".
    // This would make reconnection more consistent with normal game flow.
    TurnsMade {
        turns: Vec<TurnRecord>,
        game_status: BughouseGameStatus,
        scores: Scores,
    },
    // Used when game is ended for a reason unrelated to the last turn (flag, resign).
    GameOver {
        time: GameInstant,
        game_status: BughouseGameStatus,
        scores: Scores,
    },
    GameExportReady {
        content: String,
    },
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseClientErrorReport {
    RustPanic{ panic_info: String, backtrace: String },
    RustError{ message: String },
    UnknownError{ message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseClientEvent {
    Join {
        player_name: String,
        team: Option<Team>,
    },
    MakeTurn {
        // TODO: Add `game_id` field to avoid replaying lingering moves from older games.
        turn_input: TurnInput,
    },
    CancelPreturn,
    Resign,
    SetReady {
        is_ready: bool,
    },
    Leave,
    Reset,
    RequestExport {
        format: BughouseExportFormat,
    },
    ReportError(BughouseClientErrorReport),
}
