use serde::{Serialize, Deserialize};

use crate::clock::GameInstant;
use crate::game::{BughouseGameStatus, BughouseBoard, BughousePlayerId};
use crate::grid::Grid;
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
        scores: Vec<(Team, u32)>,
        starting_grid: Grid,
        players: Vec<(Player, BughouseBoard)>,
        time: GameInstant,          // for re-connection
        turn_log: Vec<TurnRecord>,  // for re-connection
        // TODO: Send your pending pre-turn, if any
    },
    TurnsMade(Vec<TurnRecord>),
    // Used when game is ended for a reason unrelated to the last turn (flag, resign).
    GameOver {
        time: GameInstant,
        game_status: BughouseGameStatus,
        scores: Vec<(Team, u32)>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnRecord {
    pub player_id: BughousePlayerId,
    pub turn_algebraic: String,
    pub time: GameInstant,
    pub game_status: BughouseGameStatus,
    pub scores: Vec<(Team, u32)>,
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
}
