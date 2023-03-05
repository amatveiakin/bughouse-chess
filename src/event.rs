use std::collections::HashMap;

use serde::{Serialize, Deserialize};

use crate::board::TurnInput;
use crate::chalk::{ChalkDrawing, Chalkboard};
use crate::clock::GameInstant;
use crate::game::{BughouseBoard, TurnRecord, BughouseGameStatus, PlayerInGame};
use crate::meter::MeterStats;
use crate::pgn::BughouseExportFormat;
use crate::player::{Participant, Faction};
use crate::rules::Rules;
use crate::scores::Scores;
use crate::session::Session;
use crate::starter::EffectiveStartingPosition;


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseServerRejection {
    // Cannot join: a contest with given ID does not exist.
    NoSuchContest{ contest_id: String },
    // Cannot join contest: there already is a player with this name and an active client.
    PlayerAlreadyExists{ player_name: String },
    // Server is shutting down for maintenance.
    ShuttingDown,
    // Internal error. Should be investigated.
    UnknownError{ message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseServerEvent {
    Rejection(BughouseServerRejection),
    UpdateSession {
        session: Session,
    },
    ContestWelcome {
        contest_id: String,
        rules: Rules,
    },
    LobbyUpdated {
        participants: Vec<Participant>,
    },
    // Improvement potential: Merge countdown events into `LobbyUpdated`.
    FirstGameCountdownStarted,
    FirstGameCountdownCancelled,
    // Improvement potential: Rename `GameStarted` to take reconnection into account.
    GameStarted {
        starting_position: EffectiveStartingPosition,
        players: Vec<PlayerInGame>,
        time: GameInstant,                          // for re-connection
        turn_log: Vec<TurnRecord>,                  // for re-connection
        preturns: Vec<(BughouseBoard, TurnInput)>,  // for re-connection
        game_status: BughouseGameStatus,            // for re-connection
        scores: Scores,
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
    ChalkboardUpdated {
        chalkboard: Chalkboard,
    },
    GameExportReady {
        content: String,
    },
    Pong,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BughouseClientPerformance {
    pub user_agent: String,
    pub time_zone: String,  // location estimate
    pub stats: HashMap<String, MeterStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseClientErrorReport {
    RustPanic{ panic_info: String, backtrace: String },
    RustError{ message: String },
    UnknownError{ message: String },
}

// TODO: Make sure server does not process events like MakeTurn sent during an older game.
//   This hasn't been spotted in practice so far, but seems possible in theory. Solutions:
//   - Add game_id tag to relevant events; or
//   - Implement a barrier: don't start a new game until all clients confirmed that they are
//     ready; or don't accept events for a new game until the client confirms game start.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseClientEvent {
    NewContest {
        rules: Rules,
        player_name: String,
    },
    Join {
        contest_id: String,
        player_name: String,
    },
    SetFaction {
        faction: Faction,
    },
    MakeTurn {
        board_idx: BughouseBoard,
        turn_input: TurnInput,
    },
    CancelPreturn {
        board_idx: BughouseBoard,
    },
    Resign,
    SetReady {
        is_ready: bool,
    },
    Leave,
    UpdateChalkDrawing {
        drawing: ChalkDrawing,
    },
    RequestExport {
        format: BughouseExportFormat,
    },
    ReportPerformace(BughouseClientPerformance),
    ReportError(BughouseClientErrorReport),
    Ping,
}
