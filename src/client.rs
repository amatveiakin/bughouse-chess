use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use enum_map::{enum_map, EnumMap};
use instant::Instant;
use itertools::Itertools;
use strum::IntoEnumIterator;

use crate::altered_game::{AlteredGame, WaybackDestination, WaybackState};
use crate::board::{TurnError, TurnInput, TurnMode};
use crate::chalk::{ChalkCanvas, ChalkMark, Chalkboard};
use crate::chat::{ChatMessage, ChatRecipient};
use crate::client_chat::{ClientChat, SystemMessageClass};
use crate::clock::{duration_to_mss, GameDuration, GameInstant, WallGameTimePair};
use crate::display::{get_board_index, DisplayBoard};
use crate::event::{
    BughouseClientEvent, BughouseClientPerformance, BughouseServerEvent, BughouseServerRejection,
    GameUpdate,
};
use crate::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus, BughouseParticipant,
    BughousePlayer, PlayerInGame, PlayerRelation, TurnIndex, TurnRecord, TurnRecordExpanded,
};
use crate::meter::{Meter, MeterBox, MeterStats};
use crate::pgn::BpgnExportFormat;
use crate::ping_pong::{ActiveConnectionMonitor, ActiveConnectionStatus};
use crate::player::{Faction, Participant};
use crate::role::Role;
use crate::rules::{ChessRules, DropAggression, Rules, FIRST_GAME_COUNTDOWN_DURATION};
use crate::scores::Scores;
use crate::session::Session;
use crate::starter::EffectiveStartingPosition;
use crate::{my_git_version, once_cell_regex};


#[derive(Clone, Copy, Debug)]
pub enum SubjectiveGameResult {
    Victory,
    Defeat,
    Draw,
    Observation,
}

#[derive(Clone, Debug)]
pub enum NotableEvent {
    SessionUpdated,
    MatchStarted(String), // contains MatchID
    GameStarted,
    GameOver(SubjectiveGameResult),
    TurnMade(BughouseEnvoy),
    MyReserveRestocked(BughouseBoard),
    PieceStolen,
    LowTime(BughouseBoard),
    WaybackStateUpdated(WaybackState),
    GameExportReady(String),
}

#[derive(Clone, Debug)]
pub enum EventError {
    // An action has failed. Inform the user and continue.
    Ignorable(String),
    // The client has been kicked from the match, but can rejoin.
    KickedFromMatch(String),
    // The client cannot continue operating, but *not* an internal error.
    Fatal(String),
    // Internal logic error. Should be debugged (or demoted). Could be ignored for now.
    // For non-ignorable internal errors the client would just panic.
    Internal(String),
}

#[derive(Debug)]
pub struct ServerOptions {
    pub max_starting_time: Option<Duration>,
}

#[derive(Debug)]
pub struct GameState {
    // The index of this game within the match.
    pub game_index: u64,
    // Game state including unconfirmed local changes.
    pub alt_game: AlteredGame,
    // Game start time: `None` before first move, non-`None` afterwards.
    pub time_pair: Option<WallGameTimePair>,
    // Chalk drawings from all players, including unconfirmed local marks.
    // TODO: Fix race condition: local drawings could be temporary reverted when the
    //   server sends drawings from other players. This is the same problem that we
    //   have for `is_ready` and `my_team`.
    pub chalkboard: Chalkboard,
    // Canvas for the current client to draw on.
    pub chalk_canvas: ChalkCanvas,
    // Whether wayback state is shared with other players who enabled sharing.
    shared_wayback_enabled: bool,
    // Turn index seen by those who enabled shared wayback state (regardless of whether it is
    // enabled for this client).
    shared_wayback_turn_index: Option<TurnIndex>,
    // The number of `GameUpdate`s applied so far.
    updates_applied: usize,
    // Index of the next warning in `LOW_TIME_WARNING_THRESHOLDS`.
    next_low_time_warning_idx: EnumMap<BughouseBoard, usize>,
}

#[derive(Debug)]
pub struct Match {
    pub match_id: String,
    pub my_name: String,
    pub my_faction: Faction,
    // Rules applied in every game of the match.
    pub rules: Rules,
    // All players including those not participating in the current game.
    pub participants: Vec<Participant>,
    // Scores from the past matches.
    pub scores: Option<Scores>,
    // Whether this client is ready to start a new game.
    pub is_ready: bool,
    // If `Some`, the first game is going to start after the countdown.
    pub first_game_countdown_since: Option<Instant>,
    // Active game or latest game.
    pub game_state: Option<GameState>,
    // Chat box content. Includes messages from other players and system messages.
    pub chat: ClientChat,
}

#[derive(Debug)]
enum MatchState {
    NotConnected,
    Creating { my_name: String },
    Joining { match_id: String, my_name: String },
    Connected(Match),
}

struct Connection {
    outgoing_events: VecDeque<BughouseClientEvent>,
    health_monitor: ActiveConnectionMonitor,
}

impl Connection {
    fn new(now: Instant) -> Self {
        Connection {
            outgoing_events: VecDeque::new(),
            health_monitor: ActiveConnectionMonitor::new(now),
        }
    }

    fn send(&mut self, event: BughouseClientEvent) { self.outgoing_events.push_back(event); }

    fn reset(&mut self) {
        self.outgoing_events.clear();
        self.health_monitor.reset();
    }
}

pub struct ClientState {
    user_agent: String,
    time_zone: String,
    connection: Connection,
    server_options: Option<ServerOptions>,
    match_state: MatchState,
    notable_event_queue: VecDeque<NotableEvent>,
    meter_box: MeterBox,
    ping_meter: Meter,
    session: Session,
    guest_player_name: Option<String>, // used only to create/join match
}

const LOW_TIME_WARNING_THRESHOLDS: &[Duration] = &[
    Duration::from_secs(20),
    Duration::from_secs(10),
    Duration::from_secs(5),
    Duration::from_secs(3),
    Duration::from_secs(2),
    Duration::from_secs(1),
];

macro_rules! internal_event_error {
    ($($arg:tt)*) => {
        EventError::Internal($crate::internal_error_message!($($arg)*))
    };
}

impl ClientState {
    pub fn new(user_agent: String, time_zone: String) -> Self {
        let now = Instant::now();
        let mut meter_box = MeterBox::new();
        let ping_meter = meter_box.meter("ping".to_owned());
        ClientState {
            user_agent,
            time_zone,
            connection: Connection::new(now),
            server_options: None,
            match_state: MatchState::NotConnected,
            notable_event_queue: VecDeque::new(),
            meter_box,
            ping_meter,
            session: Session::Unknown,
            guest_player_name: None,
        }
    }

    pub fn session(&self) -> &Session { &self.session }
    pub fn server_options(&self) -> Option<&ServerOptions> { self.server_options.as_ref() }
    pub fn mtch(&self) -> Option<&Match> {
        if let MatchState::Connected(ref m) = self.match_state {
            Some(m)
        } else {
            None
        }
    }
    fn mtch_mut(&mut self) -> Option<&mut Match> {
        if let MatchState::Connected(ref mut m) = self.match_state {
            Some(m)
        } else {
            None
        }
    }
    pub fn is_ready(&self) -> Option<bool> { self.mtch().map(|m| m.is_ready) }
    pub fn match_id(&self) -> Option<&String> { self.mtch().map(|m| &m.match_id) }
    pub fn game_state(&self) -> Option<&GameState> {
        self.mtch().and_then(|m| m.game_state.as_ref())
    }
    fn game_state_mut(&mut self) -> Option<&mut GameState> {
        self.mtch_mut().and_then(|m| m.game_state.as_mut())
    }
    // TODO: Reduce public mutability. This is used only for drag&drop, so limit the mutable API to that.
    pub fn alt_game_mut(&mut self) -> Option<&mut AlteredGame> {
        self.game_state_mut().map(|s| &mut s.alt_game)
    }

    pub fn my_faction(&self) -> Option<Faction> { self.mtch().map(|m| m.my_faction) }
    pub fn my_id(&self) -> Option<BughouseParticipant> {
        self.game_state().map(|s| s.alt_game.my_id())
    }
    pub fn my_name(&self) -> Option<&str> {
        match &self.match_state {
            MatchState::NotConnected { .. } => None,
            MatchState::Creating { my_name } => Some(my_name),
            MatchState::Joining { my_name, .. } => Some(my_name),
            MatchState::Connected(Match { my_name, .. }) => Some(my_name),
        }
    }
    pub fn relation_to(&self, name: &str) -> PlayerRelation {
        if self.my_name() == Some(name) {
            return PlayerRelation::Myself;
        }
        let Some(GameState { alt_game, .. }) = self.game_state() else {
            return PlayerRelation::Other;
        };
        let BughouseParticipant::Player(my_player_id) = alt_game.my_id() else {
            return PlayerRelation::Other;
        };
        let Some(other_player_id) = alt_game.game_confirmed().find_player(name) else {
            return PlayerRelation::Other;
        };
        my_player_id.relation_to(other_player_id)
    }

    pub fn first_game_countdown_left(&self) -> Option<Duration> {
        self.mtch().and_then(|m| {
            m.first_game_countdown_since.map(|t| {
                FIRST_GAME_COUNTDOWN_DURATION.saturating_sub(Instant::now().duration_since(t))
            })
        })
    }

    pub fn chalk_canvas(&self) -> Option<&ChalkCanvas> {
        self.game_state().map(|s| &s.chalk_canvas)
    }
    pub fn chalk_canvas_mut(&mut self) -> Option<&mut ChalkCanvas> {
        self.game_state_mut().map(|s| &mut s.chalk_canvas)
    }

    pub fn meter(&mut self, name: String) -> Meter { self.meter_box.meter(name) }
    pub fn read_meter_stats(&self) -> HashMap<String, MeterStats> { self.meter_box.read_stats() }
    pub fn consume_meter_stats(&mut self) -> HashMap<String, MeterStats> {
        self.meter_box.consume_stats()
    }

    pub fn got_server_welcome(&self) -> bool { self.server_options.is_some() }

    pub fn current_turnaround_time(&self) -> Duration {
        let now = Instant::now();
        self.connection.health_monitor.current_turnaround_time(now)
    }

    fn finalize_my_name_for_match(&self) -> String {
        // Let user name take priority: the user might have entered guest player name first and then
        // gone back and logged in.
        if let Some(user_info) = self.session.user_info() {
            return user_info.user_name.clone();
        }
        if let Some(guest_player_name) = &self.guest_player_name {
            return guest_player_name.clone();
        }
        panic!("Cannot determine player name: not logged in and no guest name set.");
    }
    pub fn set_guest_player_name(&mut self, player_name: Option<String>) {
        // TODO: Verify name locally and return an error instantly if it's obvious.
        // TODO: Verify name on the server and return an error if it's taken or banned.
        self.guest_player_name = player_name;
    }

    pub fn new_match(&mut self, rules: Rules) {
        let my_name = self.finalize_my_name_for_match();
        self.match_state = MatchState::Creating { my_name: my_name.clone() };
        self.connection
            .send(BughouseClientEvent::NewMatch { rules, player_name: my_name });
    }
    // Should be called in one the two situations:
    //   - Connecting to a match during a normal app flow;
    //   - Cold reconnection, when a there is no preexisting `Match` object, e.g. when the user
    //     refreshes the browser tab. If network connection was lost, but a client object is still
    //     alive, use `hot_reconnect` instead.
    pub fn join(&mut self, match_id: String) {
        let my_name = self.finalize_my_name_for_match();
        self.connection.send(BughouseClientEvent::Join {
            match_id: match_id.clone(),
            player_name: my_name.clone(),
        });
        self.match_state = MatchState::Joining { match_id, my_name };
    }
    // Hot reconnect should be called when WebSocket connection was lost due to network issues, but
    // the client object is still alive. Re-establishes connection while giving un uninterrupted
    // experience to the user. For example, it's possible to continue making and cancelling turns
    // while the connection is being restored.
    pub fn hot_reconnect(&mut self) {
        self.server_options = None;
        self.notable_event_queue.clear();
        self.connection.reset();
        if let Some(match_id) = self.match_id() {
            let my_name = self.my_name().unwrap().to_owned();
            self.connection.send(BughouseClientEvent::HotReconnect {
                match_id: match_id.clone(),
                player_name: my_name,
            });
        }
    }
    pub fn set_faction(&mut self, faction: Faction) {
        if let Some(mtch) = self.mtch_mut() {
            mtch.my_faction = faction;
            self.connection.send(BughouseClientEvent::SetFaction { faction });
        }
    }
    pub fn resign(&mut self) {
        // TODO: Display an error message if trying to resign as an observer via console.
        if self.my_faction().is_some_and(|f| f.is_player()) {
            self.connection.send(BughouseClientEvent::Resign);
        }
    }
    pub fn set_ready(&mut self, is_ready: bool) {
        if let Some(mtch) = self.mtch_mut() {
            mtch.is_ready = is_ready;
            self.connection.send(BughouseClientEvent::SetReady { is_ready });
        }
    }
    pub fn leave_match(&mut self) {
        if self.mtch().is_some() {
            self.connection.send(BughouseClientEvent::LeaveMatch);
        }
    }
    pub fn leave_server(&mut self) {
        // TODO: Do we need this? On the one hand, it's not necessary: detecting connection closure
        //   seems to work well on server. On the other hand, we cannot send it reliably from the
        //   web client when the tab is closed (especially if it was in background at that moment).
        self.connection.send(BughouseClientEvent::LeaveServer);
    }
    pub fn report_performance(&mut self) {
        let stats = self.consume_meter_stats();
        self.connection
            .send(BughouseClientEvent::ReportPerformace(BughouseClientPerformance {
                user_agent: self.user_agent.clone(),
                time_zone: self.time_zone.clone(),
                stats,
            }));
    }
    pub fn request_export(&mut self, format: BpgnExportFormat) {
        self.connection.send(BughouseClientEvent::RequestExport { format });
    }

    pub fn refresh(&mut self) {
        self.check_connection();
        self.update_low_time_warnings(true);
    }

    // Tries to execute as a "make turn" command. Returns `Some` if input was interpreted as a turn
    // command, regardless of whether the command was successful.
    //
    // Turn command consists of:
    //   1. Board notation: "<" for the left board (the only option unless double-playing), ">" for
    //      the right board.
    //   2. Algebraic turn notation or "-" to cancel pending preturn.
    //
    // Improvement potential. Add an option to treat algebraic notations as turns instead of chat
    // messages. Note that doing so by default would be a bad idea: it does make a lot of sense to
    // type algebraic notation into chat in order to hint your partner.
    pub fn execute_turn_command(&mut self, turn_command: &str) -> Option<Result<(), TurnError>> {
        let (display_board, turn) = if let Some(suffix) = turn_command.strip_prefix('<') {
            (DisplayBoard::Primary, suffix)
        } else if let Some(suffix) = turn_command.strip_prefix('>') {
            (DisplayBoard::Secondary, suffix)
        } else {
            return None;
        };
        if turn == "-" {
            self.cancel_preturn(display_board);
            Some(Ok(()))
        } else {
            Some(self.make_turn(display_board, TurnInput::Algebraic(turn.to_owned())))
        }
    }

    pub fn execute_input(&mut self, mut input: &str) {
        let command_re = once_cell_regex!("^/(\\S+)(.*)$");
        let first_word_re = once_cell_regex!("^(\\S+)(.*)$");

        if self.execute_turn_command(input).is_some() {
            return;
        }

        // TODO: Add "Send" button in UI.
        // TODO: Show recipient in UI.
        let mut recipient = if self.team_chat_enabled() {
            ChatRecipient::Team
        } else {
            ChatRecipient::All
        };
        if let Some((_, [command, argument])) =
            command_re.captures(input).map(|caps| caps.extract())
        {
            let argument = argument.trim_start();
            match command {
                "a" | "all" => {
                    recipient = ChatRecipient::All;
                    input = argument;
                }
                "dm" => {
                    let Some((_, [recipient_name, sub_argument])) =
                        first_word_re.captures(argument).map(|caps| caps.extract())
                    else {
                        // TODO: Show error.
                        return;
                    };
                    let sub_argument = sub_argument.trim_start();
                    recipient = ChatRecipient::Participant(recipient_name.to_owned());
                    input = sub_argument;
                }
                _ => {
                    self.show_command_error(format!("Unknown command: {command}"));
                    return;
                }
            }
        } else {
            // TODO: Show error.
        }
        self.send_chat_message(input.to_owned(), recipient);
    }

    pub fn show_command_result(&mut self, text: String) {
        self.add_ephemeral_system_message(SystemMessageClass::Info, text);
    }
    pub fn show_command_error(&mut self, text: String) {
        self.add_ephemeral_system_message(SystemMessageClass::Error, text);
    }

    fn make_turn_impl(
        &mut self, display_board: DisplayBoard, turn_input: TurnInput,
    ) -> Result<(), TurnError> {
        let game_state = self.game_state_mut().ok_or(TurnError::NoGameInProgress)?;
        let GameState { ref mut alt_game, time_pair, .. } = game_state;
        let board_idx = get_board_index(display_board, alt_game.perspective());
        let my_envoy = alt_game.my_id().envoy_for(board_idx).ok_or(TurnError::NotPlayer)?;
        let now = Instant::now();
        let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
        alt_game.try_local_turn(board_idx, turn_input.clone(), game_now)?;
        self.connection.send(BughouseClientEvent::MakeTurn { board_idx, turn_input });
        self.notable_event_queue.push_back(NotableEvent::TurnMade(my_envoy));
        Ok(())
    }

    pub fn make_turn(
        &mut self, display_board: DisplayBoard, turn_input: TurnInput,
    ) -> Result<(), TurnError> {
        let turn_result = self.make_turn_impl(display_board, turn_input);
        self.show_turn_result(turn_result);
        turn_result
    }

    pub fn cancel_preturn(&mut self, display_board: DisplayBoard) {
        self.show_turn_result(Ok(()));
        let Some(alt_game) = self.alt_game_mut() else {
            return;
        };
        let board_idx = get_board_index(display_board, alt_game.perspective());
        if alt_game.cancel_preturn(board_idx) {
            self.connection.send(BughouseClientEvent::CancelPreturn { board_idx });
        }
    }

    pub fn show_turn_result(&mut self, turn_result: Result<(), TurnError>) {
        let Some(mtch) = self.mtch() else {
            return;
        };
        let message = match turn_result {
            Ok(()) => None,
            Err(err) => turn_error_message(err, &mtch.rules.chess_rules),
        };
        match message {
            None => self.clear_ephemeral_chat_items(),
            Some(text) => self.add_ephemeral_system_message(SystemMessageClass::Error, text),
        }
    }

    pub fn clear_ephemeral_chat_items(&mut self) {
        let Some(mtch) = self.mtch_mut() else {
            return;
        };
        mtch.chat.remove_ephemeral();
    }
    pub fn add_ephemeral_system_message(&mut self, class: SystemMessageClass, text: String) {
        let Some(mtch) = self.mtch_mut() else {
            return;
        };
        mtch.chat.add_ephemeral_system_message(class, text);
    }

    pub fn team_chat_enabled(&self) -> bool {
        if let Some(GameState { ref alt_game, .. }) = self.game_state() {
            match alt_game.my_id() {
                BughouseParticipant::Player(BughousePlayer::SinglePlayer(_)) => true,
                // Note. If we ever allow to double-play while having a fixed team with 2+ people,
                // then message should also go to team chat by default in this case.
                BughouseParticipant::Player(BughousePlayer::DoublePlayer(_)) => false,
                BughouseParticipant::Observer => false,
            }
        } else {
            false
        }
    }
    pub fn send_chat_message(&mut self, text: String, recipient: ChatRecipient) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let team_chat_enabled = self.team_chat_enabled();
        let Some(mtch) = self.mtch_mut() else {
            return;
        };
        match &recipient {
            ChatRecipient::All => {}
            ChatRecipient::Team => {
                assert!(team_chat_enabled);
            }
            ChatRecipient::Participant(name) => {
                if !mtch.participants.iter().any(|p| p.name == *name) {
                    self.show_command_error(format!("No such player: {name}"));
                    return;
                }
            }
        };
        let message = mtch.chat.add_local(recipient, text).clone();
        self.connection.send(BughouseClientEvent::SendChatMessage { message });
    }

    pub fn add_chalk_mark(&mut self, display_board: DisplayBoard, mark: ChalkMark) {
        self.update_chalk_board(display_board, |chalkboard, my_name, board_idx| {
            chalkboard.add_mark(my_name, board_idx, mark)
        });
    }
    pub fn remove_last_chalk_mark(&mut self, display_board: DisplayBoard) {
        self.update_chalk_board(display_board, |chalkboard, my_name, board_idx| {
            chalkboard.remove_last_mark(my_name, board_idx)
        });
    }
    pub fn clear_chalk_drawing(&mut self, display_board: DisplayBoard) {
        self.update_chalk_board(display_board, |chalkboard, my_name, board_idx| {
            chalkboard.clear_drawing(my_name, board_idx);
        });
    }

    pub fn process_server_event(&mut self, event: BughouseServerEvent) -> Result<(), EventError> {
        use BughouseServerEvent::*;
        match event {
            Rejection(rejection) => self.process_rejection(rejection),
            ServerWelcome { expected_git_version, max_starting_time } => {
                self.process_server_welcome(expected_git_version, max_starting_time)
            }
            UpdateSession { session } => self.process_update_session(session),
            MatchWelcome { match_id, rules } => self.process_match_welcome(match_id, rules),
            LobbyUpdated { participants, countdown_elapsed } => {
                self.process_lobby_updated(participants, countdown_elapsed)
            }
            GameStarted {
                game_index,
                starting_position,
                players,
                time,
                updates,
                preturns,
                scores,
            } => self.process_game_started(
                game_index,
                starting_position,
                players,
                time,
                updates,
                preturns,
                scores,
            ),
            GameUpdated { updates } => self.process_game_updated(updates),
            ChatMessages { messages, confirmed_local_message_id } => {
                self.process_chat_messages(messages, confirmed_local_message_id)
            }
            ChalkboardUpdated { chalkboard } => self.process_chalkboard_updated(chalkboard),
            SharedWaybackUpdated { turn_index } => self.process_shared_wayback_updated(turn_index),
            GameExportReady { content } => self.process_game_export_ready(content),
            Pong => self.process_pong(),
        }
    }

    pub fn next_outgoing_event(&mut self) -> Option<BughouseClientEvent> {
        self.connection.outgoing_events.pop_front()
    }
    pub fn next_notable_event(&mut self) -> Option<NotableEvent> {
        self.notable_event_queue.pop_front()
    }

    fn process_rejection(&mut self, rejection: BughouseServerRejection) -> Result<(), EventError> {
        // TODO: Fix the messages containing "browser tab" for the console client.
        let error = match rejection {
            BughouseServerRejection::MaxStartingTimeExceeded { allowed, .. } => {
                EventError::Ignorable(format!(
                    "Maximum allowed starting time is {}. \
                    This is a technical limitation that we hope to relax in the future. \
                    But for now it is what it is.",
                    duration_to_mss(allowed)
                ))
            }
            BughouseServerRejection::NoSuchMatch { match_id } => {
                EventError::Ignorable(format!("Match {match_id} does not exist."))
            }
            BughouseServerRejection::PlayerAlreadyExists { player_name } => {
                EventError::Ignorable(format!(
                    "Cannot join: player {player_name} already exists. If this is you, \
                    make sure you are not connected to the same game in another browser tab. \
                    If you still can't connect, please try again in a few seconds."
                ))
            }
            BughouseServerRejection::InvalidPlayerName { player_name, reason } => {
                EventError::Ignorable(format!("Name {player_name} is invalid: {reason}"))
            }
            BughouseServerRejection::JoinedInAnotherClient => EventError::KickedFromMatch(
                "You have joined the match in another browser tab. Only one tab per \
                match can be active at a time."
                    .to_owned(),
            ),
            BughouseServerRejection::NameClashWithRegisteredUser => EventError::KickedFromMatch(
                "A registered user with the same name has joined. Registered users have \
                priority over name selection. Please choose another name and join again."
                    .to_owned(),
            ),
            BughouseServerRejection::GuestInRatedMatch => EventError::Ignorable(
                "Guests cannot join rated matches. Please register an account and join again."
                    .to_owned(),
            ),
            BughouseServerRejection::ShuttingDown => EventError::Fatal(
                "The server is shutting down for maintenance. \
                We'll be back soon (usually within 15 minutes). \
                Please come back later!"
                    .to_owned(),
            ),
            BughouseServerRejection::UnknownError { message } => EventError::Internal(message),
        };
        if matches!(error, EventError::KickedFromMatch(_)) {
            self.match_state = MatchState::NotConnected;
        }
        Err(error)
    }
    fn process_server_welcome(
        &mut self, expected_git_version: Option<String>, max_starting_time: Option<Duration>,
    ) -> Result<(), EventError> {
        if let Some(expected_git_version) = expected_git_version {
            let my_version = my_git_version!();
            if expected_git_version != my_version {
                // TODO: Send to server for logging.
                return Err(EventError::Fatal(format!(
                    "Client version ({my_version}) does not match \
                    server version ({expected_git_version}). Please refresh the page. \
                    If the problem persists, try to do a hard refresh \
                    (Ctrl+Shift+R in most browsers on Windows and Linux; \
                    Option+Cmd+E in Safari, Cmd+Shift+R in other browsers on Mac).",
                )));
            }
        }
        self.server_options = Some(ServerOptions { max_starting_time });
        // Trigger `update_session` in JS: it checks both server options and session.
        self.notable_event_queue.push_back(NotableEvent::SessionUpdated);
        Ok(())
    }
    fn process_update_session(&mut self, session: Session) -> Result<(), EventError> {
        self.session = session;
        self.notable_event_queue.push_back(NotableEvent::SessionUpdated);
        Ok(())
    }
    fn process_match_welcome(&mut self, match_id: String, rules: Rules) -> Result<(), EventError> {
        if let Some(mtch) = self.mtch_mut() {
            if mtch.match_id != match_id {
                return Err(internal_event_error!(
                    "Expected match {}, but got {match_id}",
                    mtch.match_id
                ));
            }
            assert_eq!(mtch.rules, rules);
            // TODO: Send faction and ready status if they changed while we were
            // disconnected from the server.
        } else {
            let my_name = match &self.match_state {
                MatchState::Creating { my_name } => my_name.clone(),
                MatchState::Joining { match_id: id, my_name } => {
                    if match_id != *id {
                        // Ignore: on a slow internet connection it is possible that we tried to
                        // connect to one match, went back and tried to connect to another match
                        // while the first request was still being processed.
                        return Ok(());
                    }
                    my_name.clone()
                }
                _ => return Err(internal_event_error!()),
            };
            self.notable_event_queue.push_back(NotableEvent::MatchStarted(match_id.clone()));
            // `Observer` is a safe faction default that wouldn't allow us to try acting as
            // a player if we are in fact an observer. We'll get the real faction afterwards
            // in a `LobbyUpdated` event.
            let my_faction = Faction::Observer;
            self.match_state = MatchState::Connected(Match {
                match_id,
                my_name,
                my_faction,
                rules,
                participants: Vec::new(),
                scores: None,
                is_ready: false,
                first_game_countdown_since: None,
                game_state: None,
                chat: ClientChat::new(),
            });
        }
        Ok(())
    }
    fn process_lobby_updated(
        &mut self, participants: Vec<Participant>, countdown_elapsed: Option<Duration>,
    ) -> Result<(), EventError> {
        let now = Instant::now();
        let Some(mtch) = self.mtch_mut() else {
            // This could happen if we connected to a new match and the server is still sending
            // events from the old match.
            // TODO: Find robust solution that works with all events, e.g.:
            //   - Always wait for join/leave confirmation;
            //   - Annotate each event with a unique match ID.
            return Ok(());
        };
        // TODO: Fix race condition: is_ready will toggle back and forth if a lobby update
        //   (e.g. is_ready from another player) arrived before is_ready update from this
        //   client reached the server. Same for `my_team`.
        let me = participants.iter().find(|p| p.name == mtch.my_name).unwrap();
        mtch.is_ready = me.is_ready;
        mtch.my_faction = me.faction;
        mtch.participants = participants;
        mtch.first_game_countdown_since = countdown_elapsed.map(|t| now - t);
        Ok(())
    }
    fn process_game_started(
        &mut self, game_index: u64, starting_position: EffectiveStartingPosition,
        players: Vec<PlayerInGame>, time: Option<GameInstant>, updates: Vec<GameUpdate>,
        preturns: Vec<(BughouseBoard, TurnInput)>, scores: Scores,
    ) -> Result<(), EventError> {
        let now = Instant::now();
        let mtch = self.mtch_mut().ok_or_else(|| internal_event_error!())?;
        if let Some(game_state) = mtch.game_state.as_mut() {
            if game_state.game_index == game_index {
                // This is a hot reconnect.
                for update in updates.into_iter().skip(game_state.updates_applied) {
                    // Generate notable events. A typical use-case for hot reconnect is when
                    // the user keeps playing, but WebSocket connection gets interrupted. In
                    // this case the user should hear a turn sound as soon as the connection
                    // is restored and they the opponent's turn.
                    self.apply_game_update(update, true)?;
                }
                // Improvement potential: Could remove the reborrow if we move scores update
                // from `GameOver` to a separate event and move `apply_game_update` to
                // `GameState`.
                let game_state = self.game_state().unwrap();
                if game_state.alt_game.my_id().is_player() {
                    let turns = game_state
                        .alt_game
                        .local_turns()
                        .iter()
                        .map(|t| (t.envoy.board_idx, t.turn_input.clone()))
                        .collect_vec();
                    self.connection.send(BughouseClientEvent::SetTurns { turns });
                }
                let mtch = self.mtch_mut().unwrap();
                let local_message = mtch.chat.local_messages().cloned().collect_vec();
                for message in local_message {
                    self.connection.send(BughouseClientEvent::SendChatMessage { message });
                }
                self.send_chalk_drawing_update();
                // No `NotableEvent::GameStarted`: it is used to reset the UI, while we want
                // to make reconnection experience seemless.
                return Ok(());
            }
        }
        // This is a new game or a cold reconnect.
        let time_pair = time.map(|t| WallGameTimePair::new(now, t));
        mtch.scores = Some(scores);
        let game = BughouseGame::new_with_starting_position(
            mtch.rules.clone(),
            Role::Client,
            starting_position,
            &players,
        );
        let my_id = match game.find_player(&mtch.my_name) {
            Some(id) => BughouseParticipant::Player(id),
            None => BughouseParticipant::Observer,
        };
        let alt_game = AlteredGame::new(my_id, game);
        let board_shape = alt_game.board_shape();
        let perspective = alt_game.perspective();
        mtch.game_state = Some(GameState {
            game_index,
            alt_game,
            time_pair,
            chalkboard: Chalkboard::new(),
            chalk_canvas: ChalkCanvas::new(board_shape, perspective),
            shared_wayback_enabled: false,
            shared_wayback_turn_index: None,
            updates_applied: 0,
            next_low_time_warning_idx: enum_map! { _ => 0 },
        });
        for update in updates {
            // Don't generate notable events. Cold reconnect means that the user refreshed
            // the web page or opened a new one and we had to rebuild the entire game state.
            // The user should not want to hear 50 turn sounds if 50 turns have been made so
            // far.
            self.apply_game_update(update, false)?;
        }
        for (board_idx, preturn) in preturns.into_iter() {
            let now = Instant::now();
            let game_now = GameInstant::from_pair_game_maybe_active(time_pair, now);
            // Unwrap ok: we just created the `game_state`.
            let alt_game = self.alt_game_mut().unwrap();
            // Unwrap ok: this is a preturn made by this very client before reconnection.
            let mode = alt_game.try_local_turn(board_idx, preturn, game_now).unwrap();
            assert_eq!(mode, TurnMode::Preturn);
        }
        self.notable_event_queue.push_back(NotableEvent::GameStarted);
        self.update_low_time_warnings(false);
        Ok(())
    }
    fn process_game_updated(&mut self, updates: Vec<GameUpdate>) -> Result<(), EventError> {
        for update in updates {
            self.apply_game_update(update, true)?;
        }
        Ok(())
    }
    fn process_chat_messages(
        &mut self, messages: Vec<ChatMessage>, confirmed_local_message_id: u64,
    ) -> Result<(), EventError> {
        let mtch = self.mtch_mut().ok_or_else(|| internal_event_error!())?;
        mtch.chat.remove_confirmed_local(confirmed_local_message_id);
        for message in messages {
            mtch.chat.add_static(message);
        }
        Ok(())
    }
    fn process_chalkboard_updated(&mut self, chalkboard: Chalkboard) -> Result<(), EventError> {
        let mtch = self.mtch_mut().ok_or_else(|| internal_event_error!())?;
        let game_state = mtch.game_state.as_mut().ok_or_else(|| internal_event_error!())?;
        game_state.chalkboard = chalkboard;
        Ok(())
    }
    fn process_shared_wayback_updated(
        &mut self, turn_index: Option<TurnIndex>,
    ) -> Result<(), EventError> {
        let mtch = self.mtch_mut().ok_or_else(|| internal_event_error!())?;
        let game_state = mtch.game_state.as_mut().ok_or_else(|| internal_event_error!())?;
        game_state.shared_wayback_turn_index = turn_index;
        if game_state.shared_wayback_enabled {
            _ = self.wayback_to_local(WaybackDestination::Index(turn_index), None);
        }
        Ok(())
    }
    fn process_game_export_ready(&mut self, content: String) -> Result<(), EventError> {
        self.notable_event_queue.push_back(NotableEvent::GameExportReady(content));
        Ok(())
    }
    fn process_pong(&mut self) -> Result<(), EventError> {
        let now = Instant::now();
        if let Some(ping_duration) = self.connection.health_monitor.register_pong(now) {
            self.ping_meter.record_duration(ping_duration);
        }
        Ok(())
    }

    fn apply_game_update(
        &mut self, update: GameUpdate, generate_notable_events: bool,
    ) -> Result<(), EventError> {
        let game_state = self.game_state_mut().ok_or_else(|| internal_event_error!())?;
        game_state.updates_applied += 1;
        match update {
            GameUpdate::TurnMade { turn_record } => {
                self.apply_remote_turn(turn_record, generate_notable_events)
            }
            GameUpdate::GameOver { time, game_status, scores } => {
                self.apply_game_over(time, game_status, scores, generate_notable_events)
            }
        }
    }

    fn apply_remote_turn(
        &mut self, turn_record: TurnRecord, generate_notable_events: bool,
    ) -> Result<(), EventError> {
        let TurnRecord { envoy, turn_input, time } = turn_record;
        let MatchState::Connected(mtch) = &mut self.match_state else {
            return Err(internal_event_error!());
        };
        let game_state = mtch.game_state.as_mut().ok_or_else(|| internal_event_error!())?;
        let GameState { ref mut alt_game, ref mut time_pair, .. } = game_state;
        if !alt_game.is_active() {
            return Err(internal_event_error!("Cannot make turn {:?}: game over", turn_input));
        }
        let is_my_turn = alt_game.my_id().plays_for(envoy);
        let now = Instant::now();
        if time_pair.is_none() {
            // Improvement potential. Sync client/server times better; consider NTP.
            let game_start = GameInstant::game_start();
            *time_pair = Some(WallGameTimePair::new(now, game_start));
        }
        let turn_record = alt_game.apply_remote_turn(envoy, &turn_input, time).map_err(|err| {
            internal_event_error!(
                "Got impossible turn from server: {:?}, error: {:?}",
                turn_input,
                err
            )
        })?;
        if generate_notable_events {
            if !is_my_turn {
                // The `TurnMade` event fires when a turn is seen by the user, not when it's
                // confirmed by the server.
                self.notable_event_queue.push_back(NotableEvent::TurnMade(envoy));
            }
            if participant_reserve_restocked(alt_game.my_id(), &turn_record) {
                self.notable_event_queue
                    .push_back(NotableEvent::MyReserveRestocked(envoy.board_idx.other()));
            }
            if !turn_record.turn_expanded.steals.is_empty() {
                self.notable_event_queue.push_back(NotableEvent::PieceStolen);
            }
        }
        Ok(())
    }

    fn apply_game_over(
        &mut self, game_now: GameInstant, game_status: BughouseGameStatus, scores: Scores,
        generate_notable_events: bool,
    ) -> Result<(), EventError> {
        let mtch = self.mtch_mut().ok_or_else(|| internal_event_error!())?;
        let game_state = mtch.game_state.as_mut().ok_or_else(|| internal_event_error!())?;
        let GameState { ref mut alt_game, .. } = game_state;

        mtch.scores = Some(scores);

        assert!(!game_status.is_active());
        if alt_game.is_active() {
            alt_game.set_status(game_status, game_now);
        } else {
            if game_status != alt_game.status() {
                return Err(internal_event_error!(
                    "Expected game status {:?}, got {:?}",
                    game_status,
                    alt_game.status()
                ));
            }
        }

        if generate_notable_events {
            let game_status = if let BughouseParticipant::Player(my_player_id) = alt_game.my_id() {
                match alt_game.status() {
                    BughouseGameStatus::Active => unreachable!(),
                    BughouseGameStatus::Victory(team, _) => {
                        if team == my_player_id.team() {
                            SubjectiveGameResult::Victory
                        } else {
                            SubjectiveGameResult::Defeat
                        }
                    }
                    BughouseGameStatus::Draw(_) => SubjectiveGameResult::Draw,
                }
            } else {
                SubjectiveGameResult::Observation
            };
            self.notable_event_queue.push_back(NotableEvent::GameOver(game_status));
            // Note. It would make more sense to send performanse stats on leave, but there doesn't
            // seem to be a way to do this reliably, especially on mobile.
            self.report_performance();
        }
        Ok(())
    }

    fn update_chalk_board<F>(&mut self, display_board: DisplayBoard, f: F)
    where
        F: FnOnce(&mut Chalkboard, String, BughouseBoard),
    {
        let Some(mtch) = self.mtch_mut() else {
            return;
        };
        let Some(ref mut game_state) = mtch.game_state else {
            return;
        };
        if game_state.alt_game.is_active() {
            return;
        }
        let board_idx = get_board_index(display_board, game_state.alt_game.perspective());
        f(&mut game_state.chalkboard, mtch.my_name.clone(), board_idx);
        self.send_chalk_drawing_update();
    }

    fn send_chalk_drawing_update(&mut self) {
        let Some(mtch) = self.mtch_mut() else {
            return;
        };
        let Some(ref mut game_state) = mtch.game_state else {
            return;
        };
        if game_state.alt_game.is_active() {
            return;
        }
        let drawing = game_state.chalkboard.drawings_by(&mtch.my_name).cloned().unwrap_or_default();
        self.connection.send(BughouseClientEvent::UpdateChalkDrawing { drawing })
    }

    pub fn shared_wayback_enabled(&self) -> bool {
        self.game_state().map_or(false, |s| s.shared_wayback_enabled)
    }
    pub fn set_shared_wayback(&mut self, enabled: bool) {
        let mut wayback_to_turn = None;
        if let Some(ref mut game_state) = self.game_state_mut() {
            game_state.shared_wayback_enabled = enabled;
            if enabled {
                wayback_to_turn = Some(game_state.shared_wayback_turn_index);
            }
        }
        if let Some(index) = wayback_to_turn {
            _ = self.wayback_to_local(WaybackDestination::Index(index), None);
        }
    }
    pub fn wayback_to(
        &mut self, destination: WaybackDestination, board_idx: Option<BughouseBoard>,
    ) {
        let Ok(turn_index) = self.wayback_to_local(destination, board_idx) else {
            return;
        };
        let Some(ref mut game_state) = self.game_state_mut() else {
            return;
        };
        if game_state.shared_wayback_enabled {
            game_state.shared_wayback_turn_index = turn_index;
            self.connection.send(BughouseClientEvent::SetSharedWayback { turn_index });
        }
    }
    fn wayback_to_local(
        &mut self, destination: WaybackDestination, board_idx: Option<BughouseBoard>,
    ) -> Result<Option<TurnIndex>, ()> {
        let Some(ref mut game_state) = self.game_state_mut() else {
            return Err(());
        };
        if game_state.alt_game.is_active() {
            return Err(());
        }
        let turn_index = game_state.alt_game.wayback_to(destination, board_idx);
        let wayback = game_state.alt_game.wayback_state();
        self.notable_event_queue.push_back(NotableEvent::WaybackStateUpdated(wayback));
        Ok(turn_index)
    }

    fn check_connection(&mut self) {
        use ActiveConnectionStatus::*;
        let now = Instant::now();
        match self.connection.health_monitor.update(now) {
            Noop => {}
            SendPing => {
                self.connection.send(BughouseClientEvent::Ping);
            }
        }
    }

    fn update_low_time_warnings(&mut self, generate_notable_events: bool) {
        let Some(game_state) = self.game_state_mut() else {
            return;
        };
        let GameState {
            ref alt_game,
            time_pair,
            ref mut next_low_time_warning_idx,
            ..
        } = game_state;
        let Some(time_pair) = time_pair else {
            return;
        };
        // Optimization potential. Avoid constructing `alt_game.local_game()`, which
        // involves copying the entire board, just for the sake of clock readings.
        // Note. Using `alt_game.game_confirmed()` would be a bad solution, since it
        // could lead to sound and clock visuals being out of sync. Potentially hugely
        // out of sync if local player made a move at 0:20.01 and the opponent replied
        // one minute later.
        let game_now = GameInstant::from_pair_game_active(*time_pair, Instant::now());
        let mut num_events = enum_map! { _ => 0 };
        for board_idx in BughouseBoard::iter() {
            let Some(time_left) = my_time_left(alt_game, board_idx, game_now) else {
                return;
            };
            let Ok(time_left) = Duration::try_from(time_left) else {
                return;
            };
            let idx = &mut next_low_time_warning_idx[board_idx];
            while *idx < LOW_TIME_WARNING_THRESHOLDS.len()
                && time_left <= LOW_TIME_WARNING_THRESHOLDS[*idx]
            {
                *idx += 1;
                num_events[board_idx] += 1;
            }
        }
        if generate_notable_events {
            for board_idx in BughouseBoard::iter() {
                for _ in 0..num_events[board_idx] {
                    self.notable_event_queue.push_back(NotableEvent::LowTime(board_idx));
                }
            }
        }
    }
}

fn participant_reserve_restocked(
    participant_id: BughouseParticipant, turn_record: &TurnRecordExpanded,
) -> bool {
    turn_record.turn_expanded.captures.iter().any(|c| {
        let Ok(force) = c.force.try_into() else {
            return false;
        };
        participant_id.plays_for(BughouseEnvoy {
            force,
            board_idx: turn_record.envoy.board_idx.other(),
        })
    })
}

fn my_time_left(
    alt_game: &AlteredGame, board_idx: BughouseBoard, now: GameInstant,
) -> Option<GameDuration> {
    alt_game
        .my_id()
        .envoy_for(board_idx)
        .map(|e| alt_game.local_game().board(e.board_idx).clock().time_left(e.force, now))
}

// Improvement potential. Add TurnError payload to make error messages even more useful.
fn turn_error_message(err: TurnError, rules: &ChessRules) -> Option<String> {
    // We return `None` for errors that are either internal or trivial.
    // Improvement potential. Show even trivial errors (like PathBlocked) when making a turn via
    //   algebraic notation.
    let bughouse_rules = || rules.bughouse_rules.as_ref().unwrap();
    let drop_aggression = || match bughouse_rules().drop_aggression {
        DropAggression::NoCheck => "Cannot drop pieces with a check",
        DropAggression::NoChessMate => {
            "Cannot drop pieces with a checkmate (according to chess rules)"
        }
        DropAggression::NoBughouseMate => {
            "Cannot drop pieces with a checkmate (according to bughouse rules)"
        }
        DropAggression::MateAllowed => unreachable!(),
    };
    match err {
        TurnError::NotPlayer => None,
        TurnError::DontControlPiece => None,
        TurnError::WrongTurnMode => None,
        TurnError::InvalidNotation => Some("Invalid notation.".to_owned()),
        TurnError::AmbiguousNotation => Some("Ambiguous notation.".to_owned()),
        TurnError::CaptureNotationRequiresCapture => {
            Some("Capture notation (“x”) requires capture.".to_owned())
        }
        TurnError::PieceMissing => Some("Piece is missing.".to_owned()),
        TurnError::PreturnLimitReached => None,
        TurnError::ImpossibleTrajectory => None,
        TurnError::PathBlocked => None,
        TurnError::UnprotectedKing => Some("King is unprotected.".to_owned()),
        TurnError::CastlingPieceHasMoved => Some("Cannot castle: piece has moved.".to_owned()),
        TurnError::CannotCastleDroppedKing => Some("Cannot castle: king was dropped.".to_owned()),
        TurnError::BadPromotionType => Some(format!(
            "Bad promotion type, expected “{}”",
            rules.promotion().to_human_readable()
        )),
        TurnError::MustPromoteHere => Some("Missing pawn promotion".to_owned()),
        TurnError::CannotPromoteHere => Some("Cannot promote here".to_owned()),
        TurnError::InvalidUpgradePromotionTarget => Some("Invalid promotion target".to_owned()),
        TurnError::InvalidStealPromotionTarget => Some("Invalid steal target".to_owned()),
        TurnError::DropRequiresBughouse => None,
        TurnError::DropPieceMissing => Some("Reserve piece is missing.".to_owned()),
        TurnError::InvalidPawnDropRank => Some(format!(
            "Pawns must be dropped on ranks {} from the player",
            bughouse_rules().pawn_drop_ranks.to_human_readable()
        )),
        TurnError::DropBlocked => None,
        TurnError::DropAggression => Some(drop_aggression().to_owned()),
        TurnError::StealTargetMissing => Some("Steal target is missing.".to_owned()),
        TurnError::StealTargetInvalid => Some("Steal target is invalid.".to_owned()),
        TurnError::ExposingKingByStealing => Some("Cannot expose king by stealing.".to_owned()),
        TurnError::ExposingPartnerKingByStealing => {
            Some("Cannot expose partner king by stealing.".to_owned())
        }
        TurnError::NotDuckChess => Some("Not duck chess.".to_owned()),
        TurnError::DuckPlacementIsSpecialTurnKind => None,
        TurnError::MustMovePieceBeforeDuck => {
            Some("Must move your own piece before the duck.".to_owned())
        }
        TurnError::MustPlaceDuck => Some("Must place the duck.".to_owned()),
        TurnError::MustChangeDuckPosition => {
            Some("Must move duck to a different position".to_owned())
        }
        TurnError::KingCannotCaptureInAtomicChess => {
            Some("King cannot capture in atomic chess".to_owned())
        }
        TurnError::MustDropKingIfPossible => {
            Some("Must drop a king when you have one in reserve".to_owned())
        }
        TurnError::NoTurnInProgress => None,
        TurnError::TurnObsolete => None,
        TurnError::PreviousTurnNotFinished => None,
        TurnError::Defunct => None,
        TurnError::Cancelled => None,
        TurnError::NoGameInProgress => None,
        TurnError::GameOver => None,
        TurnError::WaybackIsActive => None,
    }
}
