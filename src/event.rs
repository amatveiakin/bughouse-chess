use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::board::TurnInput;
use crate::chalk::{ChalkDrawing, Chalkboard};
use crate::chat::{ChatMessage, OutgoingChatMessage};
use crate::clock::GameInstant;
use crate::game::{BughouseBoard, BughouseGameStatus, PlayerInGame, TurnIndex, TurnRecord};
use crate::meter::MeterStats;
use crate::pgn::BpgnExportFormat;
use crate::player::{Faction, Participant};
use crate::rules::Rules;
use crate::scores::Scores;
use crate::session::Session;
use crate::starter::EffectiveStartingPosition;


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseServerRejection {
    MaxStartingTimeExceeded { requested: Duration, allowed: Duration },
    // Cannot join: a match with given ID does not exist.
    NoSuchMatch { match_id: String },
    // Cannot join match: there already is a player with this name and an active client.
    PlayerAlreadyExists { player_name: String },
    // Cannot create account or join as a guest with a given name.
    InvalidPlayerName { player_name: String, reason: String },
    // Registered user kicked out of a match, because they joined in another client (e.g. another
    // browser tab). We never send this for guest users, because we cannot be sure if it's them or not.
    JoinedInAnotherClient,
    // Guest user kicked out of a match, because a registered user with the same name has joined.
    NameClashWithRegisteredUser,
    // Trying to participate in a rated match with a guest account.
    GuestInRatedMatch,
    // Server is shutting down for maintenance.
    ShuttingDown,
    // Internal error. Should be investigated.
    UnknownError { message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GameUpdate {
    TurnMade {
        turn_record: TurnRecord,
    },
    // Sent when game is ended for any reason. If the game ended due to checkmate, must be sent
    // together with the corresponding `TurnMade` event and `game_status` must match the status
    // resulting from the turn (in other words, registered turn always taked priority over flag,
    // resigns, etc.). Cannot be followed by `TurnMade` events.
    GameOver {
        time: GameInstant,
        game_status: BughouseGameStatus,
        scores: Scores,
    },
}

// Improvement potential. Automatically bundle all event generated during a single cycle into one
// message. This would:
//   - Make sure that all updates are atomic: e.g. this would remove the gap between `MatchWelcome`
//     and getting the faction in `LobbyUpdated`; or this prevent the user from sending a local
//     message before getting the proper local message ID from `ChatMessages`.
//   - Allow to stop bunding events manually, as in `GameUpdated` and `GameStarted`.
//   - Allow to remove duplication resulting from the manual bundling.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseServerEvent {
    Rejection(BughouseServerRejection),
    ServerWelcome {
        expected_git_version: Option<String>,
        max_starting_time: Option<Duration>,
    },
    UpdateSession {
        session: Session,
    },
    MatchWelcome {
        match_id: String,
        rules: Rules,
    },
    LobbyUpdated {
        participants: Vec<Participant>,
        countdown_elapsed: Option<Duration>,
    },
    // Improvement potential: Rename `GameStarted` to take reconnection into account.
    GameStarted {
        game_index: u64,
        starting_position: EffectiveStartingPosition,
        players: Vec<PlayerInGame>,
        time: Option<GameInstant>,                 // for re-connection
        updates: Vec<GameUpdate>,                  // for re-connection
        preturns: Vec<(BughouseBoard, TurnInput)>, // for re-connection
        // It's a bit weird that we send the scores two times (here and in `GameUpdate::GameOver`)
        // for finished games, but not really problematic. And we do need it for unfinished games.
        scores: Scores,
    },
    GameUpdated {
        updates: Vec<GameUpdate>,
    },
    ChatMessages {
        messages: Vec<ChatMessage>,
        confirmed_local_message_id: u64,
    },
    ChalkboardUpdated {
        chalkboard: Chalkboard,
    },
    SharedWaybackUpdated {
        turn_index: Option<TurnIndex>,
    },
    GameExportReady {
        content: String,
    },
    Pong,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BughouseClientPerformance {
    pub user_agent: String,
    pub time_zone: String, // location estimate
    pub stats: HashMap<String, MeterStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseClientErrorReport {
    RustPanic { panic_info: String, backtrace: String },
    RustError { message: String },
    UnknownError { message: String },
}

// TODO: Make sure server does not process events like MakeTurn sent during an older game.
//   This hasn't been spotted in practice so far, but seems possible in theory. Solutions:
//   - Add game_index tag to relevant events; or
//   - Implement a barrier: don't start a new game until all clients confirmed that they are
//     ready; or don't accept events for a new game until the client confirms game start.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BughouseClientEvent {
    NewMatch {
        rules: Rules,
        player_name: String,
    },
    Join {
        match_id: String,
        player_name: String,
    },
    HotReconnect {
        match_id: String,
        player_name: String,
    },
    SetFaction {
        faction: Faction,
    },
    SetTurns {
        turns: Vec<(BughouseBoard, TurnInput)>,
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
    LeaveMatch,
    LeaveServer,
    SendChatMessage {
        message: OutgoingChatMessage,
    },
    UpdateChalkDrawing {
        drawing: ChalkDrawing,
    },
    SetSharedWayback {
        turn_index: Option<TurnIndex>,
    },
    RequestExport {
        format: BpgnExportFormat,
    },
    ReportPerformace(BughouseClientPerformance),
    ReportError(BughouseClientErrorReport),
    Ping,
}
