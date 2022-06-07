use serde::{Serialize, Deserialize};

use crate::clock::GameInstant;
use crate::game::{TurnRecord, BughouseGameStatus, BughouseBoard};
use crate::grid::Grid;
use crate::pgn::BughouseExportFormat;
use crate::player::{Player, Team};
use crate::rules::{ChessRules, BughouseRules};


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseServerEvent {
    Error {
        message: String,
    },
    LobbyUpdated {
        players: Vec<Player>
    },
    GameStarted {  // TODO: Rename to take reconnection into account
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        starting_grid: Grid,
        players: Vec<(Player, BughouseBoard)>,
        time: GameInstant,                // for re-connection
        turn_log: Vec<TurnRecord>,        // for re-connection
        game_status: BughouseGameStatus,  // for re-connection
        scores: Vec<(Team, u32)>,
        // TODO: Send your pending pre-turn, if any
    },
    // Improvement potential: unite `TurnsMade` and `GameOver` into a single event "something happened".
    // This would make reconnection more consistent with normal game flow.
    TurnsMade {
        turns: Vec<TurnRecord>,
        game_status: BughouseGameStatus,
        scores: Vec<(Team, u32)>,
    },
    // Used when game is ended for a reason unrelated to the last turn (flag, resign).
    GameOver {
        time: GameInstant,
        game_status: BughouseGameStatus,
        scores: Vec<(Team, u32)>,
    },
    GameExportReady {
        content: String,
    },
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseClientEvent {
    Join {
        player_name: String,
        team: Team,
    },
    MakeTurn {
        // TODO: Add `game_id` field to avoid replaying lingering moves from older games.
        turn_algebraic: String,
    },
    CancelPreturn,
    Resign,
    NextGame,
    Leave,
    Reset,
    RequestExport {
        format: BughouseExportFormat,
    },
}
