use serde::{Serialize, Deserialize};

use crate::clock::GameInstant;
use crate::game::{BughouseGameStatus, BughouseBoard};
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
    GameStarted {
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        starting_grid: Grid,
        players: Vec<(Player, BughouseBoard)>,
    },
    TurnMade {
        player_name: String,
        turn_algebraic: String,
        time: GameInstant,
        game_status: BughouseGameStatus,
    },
    // Used when game is ended for a reason unrelated to the last turn (flag, resign).
    GameOver {
        time: GameInstant,
        game_status: BughouseGameStatus,
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
    Resign,
    Leave,
}
