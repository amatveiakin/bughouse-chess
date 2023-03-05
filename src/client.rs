use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::time::Duration;

use enum_map::{EnumMap, enum_map};
use instant::Instant;
use strum::{IntoEnumIterator};

use crate::altered_game::AlteredGame;
use crate::board::{TurnError, TurnMode, TurnInput};
use crate::chalk::{Chalkboard, ChalkCanvas, ChalkDrawing, ChalkMark};
use crate::clock::{GameInstant, WallGameTimePair};
use crate::display::{DisplayBoard, get_board_index};
use crate::game::{
    TurnRecord, BughouseBoard, BughouseEnvoy, BughousePlayer, BughouseParticipant, PlayerRelation,
    BughouseGameStatus, BughouseGame
};
use crate::event::{BughouseServerEvent, BughouseClientEvent, BughouseServerRejection, BughouseClientPerformance};
use crate::meter::{Meter, MeterBox, MeterStats};
use crate::pgn::BughouseExportFormat;
use crate::ping_pong::{ActiveConnectionMonitor, ActiveConnectionStatus};
use crate::player::{Participant, Faction};
use crate::rules::{Rules, FIRST_GAME_COUNTDOWN_DURATION};
use crate::session::Session;
use crate::scores::Scores;


#[derive(Clone, Copy, Debug)]
pub enum SubjectiveGameResult {
    Victory,
    Defeat,
    Draw,
}

#[derive(Clone, Debug)]
pub enum NotableEvent {
    ContestStarted(String),  // contains ContestID
    GameStarted,
    GameOver(SubjectiveGameResult),
    TurnMade(BughouseEnvoy),
    MyReserveRestocked,
    LowTime,
    GameExportReady(String),
}

#[derive(Clone, Debug)]
pub enum EventError {
    // An action has failed. Inform the user and continue.
    IgnorableError(String),
    // The client cannot continue operating, but *not* an internal error.
    FatalError(String),
    // Internal logic error. Should be debugged (or demoted). Could be ignored for now.
    // For non-ignorable internal errors the client would just panic.
    InternalEvent(String),
}

#[derive(Debug)]
pub struct GameState {
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
    // Index of the next warning in `LOW_TIME_WARNING_THRESHOLDS`.
    next_low_time_warning_idx: EnumMap<BughouseBoard, usize>,

}

#[derive(Debug)]
pub struct Contest {
    pub contest_id: String,
    pub my_name: String,
    pub my_faction: Faction,
    // Rules applied in every game of the contest.
    pub rules: Rules,
    // All players including those not participating in the current game.
    pub participants: Vec<Participant>,
    // Scores from the past matches.
    pub scores: Scores,
    // Whether this client is ready to start a new game.
    pub is_ready: bool,
    // If `Some`, the first game is going to start after the countdown.
    pub first_game_countdown_since: Option<Instant>,
    // Active game or latest game.
    pub game_state: Option<GameState>,
}

#[derive(Debug)]
enum ContestState {
    NotConnected,
    Creating {
        my_name: String,
    },
    Joining {
        contest_id: String,
        my_name: String,
    },
    Connected(Contest),
}

struct Connection {
    events_tx: mpsc::Sender<BughouseClientEvent>,
    health_monitor: ActiveConnectionMonitor,
}

impl Connection {
    fn new(events_tx: mpsc::Sender<BughouseClientEvent>) -> Self {
        Connection {
            events_tx,
            health_monitor: ActiveConnectionMonitor::new(),
        }
    }

    fn send(&mut self, event: BughouseClientEvent) {
        self.events_tx.send(event).unwrap();
    }
}

pub struct ClientState {
    user_agent: String,
    time_zone: String,
    connection: Connection,
    contest_state: ContestState,
    notable_event_queue: VecDeque<NotableEvent>,
    meter_box: MeterBox,
    ping_meter: Meter,
    session: Session,
}

const LOW_TIME_WARNING_THRESHOLDS: &[Duration] = &[
    Duration::from_secs(20),
    Duration::from_secs(10),
    Duration::from_secs(5),
    Duration::from_secs(3),
    Duration::from_secs(2),
    Duration::from_secs(1),
];

macro_rules! internal_error {
    ($($arg:tt)*) => {
        EventError::InternalEvent(format!($($arg)*))
    }
}

impl ClientState {
    pub fn new(
        user_agent: String, time_zone: String, events_tx: mpsc::Sender<BughouseClientEvent>
    ) -> Self {
        let mut meter_box = MeterBox::new();
        let ping_meter = meter_box.meter("ping".to_owned());
        ClientState {
            user_agent,
            time_zone,
            connection: Connection::new(events_tx),
            contest_state: ContestState::NotConnected,
            notable_event_queue: VecDeque::new(),
            meter_box,
            ping_meter,
            session: Session::default(),
        }
    }

    pub fn contest(&self) -> Option<&Contest> {
        if let ContestState::Connected(ref c) = self.contest_state { Some(c) } else { None }
    }
    fn contest_mut(&mut self) -> Option<&mut Contest> {
        if let ContestState::Connected(ref mut c) = self.contest_state { Some(c) } else { None }
    }
    pub fn is_ready(&self) -> Option<bool> { self.contest().map(|c| c.is_ready) }
    pub fn contest_id(&self) -> Option<&String> { self.contest().map(|c| &c.contest_id) }
    pub fn game_state(&self) -> Option<&GameState> { self.contest().and_then(|c| c.game_state.as_ref()) }
    fn game_state_mut(&mut self) -> Option<&mut GameState> { self.contest_mut().and_then(|c| c.game_state.as_mut()) }
    // TODO: Reduce public mutability. This is used only for drag&drop, so limit the mutable API to that.
    pub fn alt_game_mut(&mut self) -> Option<&mut AlteredGame> { self.game_state_mut().map(|s| &mut s.alt_game) }

    pub fn my_faction(&self) -> Option<Faction> { self.contest().map(|c| c.my_faction) }
    pub fn my_id(&self) -> Option<BughouseParticipant> { self.game_state().map(|s| s.alt_game.my_id()) }
    pub fn my_name(&self) -> Option<&str> {
        match &self.contest_state {
            ContestState::NotConnected => None,
            ContestState::Creating{ my_name } => Some(&my_name),
            ContestState::Joining{ my_name, .. } => Some(&my_name),
            ContestState::Connected(Contest{ my_name, .. }) => Some(&my_name),
        }
    }
    pub fn relation_to(&self, name: &str) -> PlayerRelation {
        if self.my_name() == Some(name) {
            return PlayerRelation::Myself;
        }
        let Some(GameState{ alt_game, .. }) = self.game_state() else {
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
        self.contest().and_then(|c| c.first_game_countdown_since.map(|t| {
            FIRST_GAME_COUNTDOWN_DURATION.saturating_sub(Instant::now().duration_since(t))
        }))
    }

    pub fn chalk_canvas(&self) -> Option<&ChalkCanvas> { self.game_state().map(|s| &s.chalk_canvas) }
    pub fn chalk_canvas_mut(&mut self) -> Option<&mut ChalkCanvas> { self.game_state_mut().map(|s| &mut s.chalk_canvas) }

    pub fn meter(&mut self, name: String) -> Meter { self.meter_box.meter(name) }
    pub fn read_meter_stats(&self) -> HashMap<String, MeterStats> { self.meter_box.read_stats() }
    pub fn consume_meter_stats(&mut self) -> HashMap<String, MeterStats> { self.meter_box.consume_stats() }

    pub fn current_turnaround_time(&self) -> Option<Duration> {
        let now = Instant::now();
        self.connection.health_monitor.current_turnaround_time(now)
    }

    pub fn new_contest(&mut self, rules: Rules, my_name: String) {
        self.contest_state = ContestState::Creating{ my_name: my_name.clone() };
        self.connection.send(BughouseClientEvent::NewContest{ rules, player_name: my_name });
    }
    pub fn join(&mut self, contest_id: String, my_name: String) {
        self.connection.send(BughouseClientEvent::Join {
            contest_id: contest_id.clone(),
            player_name: my_name.clone(),
        });
        self.contest_state = ContestState::Joining{ contest_id, my_name };
    }
    pub fn set_faction(&mut self, faction: Faction) {
        if let Some(contest) = self.contest_mut() {
            contest.my_faction = faction;
            self.connection.send(BughouseClientEvent::SetFaction{ faction });
        }
    }
    pub fn resign(&mut self) {
        self.connection.send(BughouseClientEvent::Resign);
    }
    pub fn set_ready(&mut self, is_ready: bool) {
        if let Some(contest) = self.contest_mut() {
            contest.is_ready = is_ready;
            self.connection.send(BughouseClientEvent::SetReady{ is_ready });
        }
    }
    pub fn leave(&mut self) {
        // TODO: Do we need this? On the one hand, it's not necessary: detecting connection closure
        //   seems to work well on server. On the other hand, we cannot send it reliably from the web
        //   client when the tab is closed (especially if it was in background at that moment).
        self.connection.send(BughouseClientEvent::Leave);
    }
    pub fn report_performance(&mut self) {
        let stats = self.consume_meter_stats();
        self.connection.send(BughouseClientEvent::ReportPerformace(BughouseClientPerformance {
            user_agent: self.user_agent.clone(),
            time_zone: self.time_zone.clone(),
            stats,
        }));
    }
    pub fn request_export(&mut self, format: BughouseExportFormat) {
        self.connection.send(BughouseClientEvent::RequestExport{ format });
    }

    pub fn refresh(&mut self) {
        self.check_connection();
        self.update_low_time_warnings(true);
    }

    // Turn command consists of:
    //   1. Board notation (usually optional; mandatory if double-playing).
    //   2. Algebraic turn notation or "-" to cancel pending preturn.
    pub fn execute_turn_command(&mut self, turn_command: &str) -> Result<(), TurnError> {
        let (display_board, turn) =
            if let Some(suffix) = turn_command.strip_prefix('<') {
                (DisplayBoard::Primary, suffix)
            } else if let Some(suffix) = turn_command.strip_prefix('>') {
                (DisplayBoard::Secondary, suffix)
            } else {
                let game_state = self.game_state().ok_or(TurnError::NoGameInProgress)?;
                let my_player_id = game_state.alt_game.my_id().as_player().ok_or(TurnError::NotPlayer)?;
                match my_player_id {
                    BughousePlayer::SinglePlayer(_) => (DisplayBoard::Primary, turn_command),
                    BughousePlayer::DoublePlayer(_) => { return Err(TurnError::AmbiguousBoard); },
                }
            };
        if turn == "-" {
            self.cancel_preturn(display_board);
        } else {
            self.make_turn(display_board, TurnInput::Algebraic(turn.to_owned()))?;
        }
        Ok(())
    }

    pub fn make_turn(&mut self, display_board: DisplayBoard, turn_input: TurnInput)
        -> Result<(), TurnError>
    {
        let game_state = self.game_state_mut().ok_or(TurnError::NoGameInProgress)?;
        let GameState{ ref mut alt_game, time_pair, .. } = game_state;
        let board_idx = get_board_index(display_board, alt_game.perspective());
        let my_envoy = alt_game.my_id().envoy_for(board_idx).ok_or(TurnError::NotPlayer)?;
        let now = Instant::now();
        let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
        alt_game.try_local_turn(board_idx, turn_input.clone(), game_now)?;
        self.connection.send(BughouseClientEvent::MakeTurn{ board_idx, turn_input });
        self.notable_event_queue.push_back(NotableEvent::TurnMade(my_envoy));
        Ok(())
    }

    pub fn cancel_preturn(&mut self, display_board: DisplayBoard) {
        let Some(alt_game) = self.alt_game_mut() else {
            return;
        };
        let board_idx = get_board_index(display_board, alt_game.perspective());
        if alt_game.cancel_preturn(board_idx) {
            self.connection.send(BughouseClientEvent::CancelPreturn{ board_idx });
        }
    }

    pub fn add_chalk_mark(&mut self, display_board: DisplayBoard, mark: ChalkMark) {
        self.update_chalk_board(
            display_board,
            |chalkboard, my_name, board_idx| chalkboard.add_mark(my_name, board_idx, mark)
        );
    }
    pub fn remove_last_chalk_mark(&mut self, display_board: DisplayBoard) {
        self.update_chalk_board(
            display_board,
            |chalkboard, my_name, board_idx| chalkboard.remove_last_mark(my_name, board_idx)
        );
    }
    pub fn clear_chalk_drawing(&mut self, display_board: DisplayBoard) {
        self.update_chalk_board(
            display_board,
            |chalkboard, my_name, board_idx| { chalkboard.clear_drawing(my_name, board_idx); }
        );
    }

    // Improvement potential: Split into functions like `CoreServerState.on_client_event` does.
    pub fn process_server_event(&mut self, event: BughouseServerEvent) -> Result<(), EventError> {
        use BughouseServerEvent::*;
        let now = Instant::now();
        match event {
            Rejection(rejection) => {
                match rejection {
                    BughouseServerRejection::PlayerAlreadyExists{ player_name } => {
                        // TODO: Fix the message ("browser tab" part) for the console client.
                        return Err(EventError::IgnorableError(format!("\
                            Cannot join: player {player_name} already exists. If this is you, \
                            make sure you are not connected to the same game in another browser tab. \
                            If you still can't connect, please try again in a few seconds.\
                        ")));
                    },
                    BughouseServerRejection::NoSuchContest{ contest_id } => {
                        return Err(EventError::IgnorableError(format!(
                            "Contest {contest_id} does not exist."
                        )));
                    },
                    BughouseServerRejection::ShuttingDown => {
                        return Err(EventError::FatalError("\
                            The server is shutting down for maintenance. \
                            We'll be back soon (usually within 15 minutes). \
                            Please come back later!\
                        ".to_owned()));
                    },
                    BughouseServerRejection::UnknownError{ message } => {
                        return Err(internal_error!("Got error from server: {}", message));
                    },
                }
            },
            UpdateSession { session } => {
                self.session = session;
            },
            ContestWelcome{ contest_id, rules } => {
                let my_name = match &self.contest_state {
                    ContestState::Creating{ my_name } => {
                        my_name.clone()
                    }
                    ContestState::Joining{ contest_id: id, my_name } => {
                        if contest_id != *id {
                            return Err(internal_error!("Cannot apply ContestWelcome: expected contest {id}, but got {contest_id}"));
                        }
                        my_name.clone()
                    },
                    _ => return Err(internal_error!("Cannot apply ContestWelcome: not expecting a new contest")),
                };
                self.notable_event_queue.push_back(NotableEvent::ContestStarted(contest_id.clone()));
                // `Observer` is a safe faction default that wouldn't allow us to try acting as
                // a player if we are in fact an observer. We'll get the real faction afterwards
                // in a `LobbyUpdated` event.
                let my_faction = Faction::Observer;
                self.contest_state = ContestState::Connected(Contest {
                    contest_id,
                    my_name,
                    my_faction,
                    rules,
                    participants: Vec::new(),
                    scores: Scores::new(),
                    is_ready: false,
                    first_game_countdown_since: None,
                    game_state: None,
                });
            },
            LobbyUpdated{ participants } => {
                let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot apply LobbyUpdated: no contest in progress"))?;
                // TODO: Fix race condition: is_ready will toggle back and forth if a lobby update
                //   (e.g. is_ready from another player) arrived before is_ready update from this
                //   client reached the server. Same for `my_team`.
                let me = participants.iter().find(|p| p.name == contest.my_name).unwrap();
                contest.is_ready = me.is_ready;
                contest.my_faction = me.faction;
                contest.participants = participants;
            },
            FirstGameCountdownStarted => {
                let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot apply FirstGameCountdownStarted: no contest in progress"))?;
                contest.first_game_countdown_since = Some(now);
            },
            FirstGameCountdownCancelled => {
                let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot apply FirstGameCountdownCancelled: no contest in progress"))?;
                contest.first_game_countdown_since = None;
            },
            GameStarted{ starting_position, players, time, turn_log, preturns, game_status, scores } => {
                let time_pair = if turn_log.is_empty() {
                    assert!(time.elapsed_since_start().is_zero());
                    None
                } else {
                    Some(WallGameTimePair::new(now, time.approximate()))
                };
                let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot apply GameStarted: no contest in progress"))?;
                let game = BughouseGame::new_with_starting_position(
                    contest.rules.contest_rules.clone(),
                    contest.rules.chess_rules.clone(),
                    contest.rules.bughouse_rules.clone(),
                    starting_position,
                    &players
                );
                let my_id = match game.find_player(&contest.my_name) {
                    Some(id) => BughouseParticipant::Player(id),
                    None => BughouseParticipant::Observer,
                };
                let alt_game = AlteredGame::new(my_id, game);
                let perspective = alt_game.perspective();
                contest.game_state = Some(GameState {
                    alt_game,
                    time_pair,
                    chalkboard: Chalkboard::new(),
                    chalk_canvas: ChalkCanvas::new(perspective),
                    next_low_time_warning_idx: enum_map!{ _ => 0 },
                });
                for turn in turn_log {
                    self.apply_remote_turn(turn, false)?;
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
                self.update_game_status(game_status, time)?;
                self.update_scores(scores)?;
                self.notable_event_queue.push_back(NotableEvent::GameStarted);
                self.update_low_time_warnings(false);
            },
            TurnsMade{ turns, game_status, scores } => {
                for turn in turns {
                    self.apply_remote_turn(turn, true)?;
                }
                self.verify_game_status(game_status)?;
                self.update_scores(scores)?;
                if game_status != BughouseGameStatus::Active {
                    self.game_over_postprocess()?;
                }
            },
            GameOver{ time, game_status, scores: new_scores } => {
                let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot apply GameOver: no contest in progress"))?;
                let game_state = contest.game_state.as_mut().ok_or_else(|| internal_error!("Cannot apply GameOver: no game in progress"))?;
                assert!(game_state.alt_game.status() == BughouseGameStatus::Active);
                game_state.alt_game.set_status(game_status, time);
                contest.scores = new_scores;
                self.game_over_postprocess()?;
            },
            ChalkboardUpdated{ chalkboard } => {
                let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot apply ChalkboardUpdated: no contest in progress"))?;
                let game_state = contest.game_state.as_mut().ok_or_else(|| internal_error!("Cannot apply ChalkboardUpdated: no game in progress"))?;
                game_state.chalkboard = chalkboard;
            },
            GameExportReady{ content } => {
                self.notable_event_queue.push_back(NotableEvent::GameExportReady(content));
            },
            Pong => {
                if let Some(ping_duration) = self.connection.health_monitor.register_pong(now) {
                    self.ping_meter.record_duration(ping_duration);
                }
            }
        }
        Ok(())
    }

    pub fn next_notable_event(&mut self) -> Option<NotableEvent> {
        self.notable_event_queue.pop_front()
    }

    fn apply_remote_turn(
        &mut self, turn_record: TurnRecord, generate_notable_events: bool
    ) -> Result<(), EventError> {
        let TurnRecord{ envoy, turn_algebraic, time } = turn_record;
        let ContestState::Connected(contest) = &mut self.contest_state else {
            return Err(internal_error!("Cannot make turn: no contest in progress"));
        };
        let game_state = contest.game_state.as_mut().ok_or_else(|| internal_error!("Cannot make turn: no game in progress"))?;
        let GameState{ ref mut alt_game, ref mut time_pair, .. } = game_state;
        if alt_game.status() != BughouseGameStatus::Active {
            return Err(internal_error!("Cannot make turn {}: game over", turn_algebraic));
        }
        let is_my_turn = alt_game.my_id().plays_for(envoy);
        let now = Instant::now();
        if time_pair.is_none() {
            // Improvement potential. Sync client/server times better; consider NTP.
            let game_start = GameInstant::game_start().approximate();
            *time_pair = Some(WallGameTimePair::new(now, game_start));
        }
        let old_reserve_size = reserve_size(alt_game, envoy);
        alt_game.apply_remote_turn_algebraic(
            envoy, &turn_algebraic, time
        ).map_err(|err| {
            internal_error!("Got impossible turn from server: {}, error: {:?}", turn_algebraic, err)
        })?;
        let new_reserve_size = reserve_size(alt_game, envoy);
        if generate_notable_events {
            if !is_my_turn {
                // The `TurnMade` event fires when a turn is seen by the user, not when it's
                // confirmed by the server.
                self.notable_event_queue.push_back(NotableEvent::TurnMade(envoy));
            }
            if is_my_turn && new_reserve_size > old_reserve_size {
                self.notable_event_queue.push_back(NotableEvent::MyReserveRestocked);
            }
        }
        Ok(())
    }

    fn verify_game_status(&mut self, game_status: BughouseGameStatus) -> Result<(), EventError> {
        let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot verify game status: no contest in progress"))?;
        let game_state = contest.game_state.as_mut().ok_or_else(|| internal_error!("Cannot verify game status: no game in progress"))?;
        let GameState{ ref mut alt_game, .. } = game_state;
        if game_status != alt_game.status() {
            return Err(internal_error!(
                "Expected game status {:?}, got {:?}", game_status, alt_game.status()
            ));
        }
        Ok(())
    }

    fn update_game_status(&mut self, game_status: BughouseGameStatus, game_now: GameInstant)
        -> Result<(), EventError>
    {
        let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot update game status: no contest in progress"))?;
        let game_state = contest.game_state.as_mut().ok_or_else(|| internal_error!("Cannot update game status: no game in progress"))?;
        let GameState{ ref mut alt_game, .. } = game_state;
        if alt_game.status() == BughouseGameStatus::Active {
            if game_status != BughouseGameStatus::Active {
                alt_game.set_status(game_status, game_now);
            }
            Ok(())
        } else {
            self.verify_game_status(game_status)
        }
    }

    fn game_over_postprocess(&mut self) -> Result<(), EventError> {
        let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot process game over: no contest in progress"))?;
        let game_state = contest.game_state.as_mut().ok_or_else(|| internal_error!("Cannot process game over: no game in progress"))?;
        let GameState{ ref mut alt_game, .. } = game_state;
        if let BughouseParticipant::Player(my_player_id) = alt_game.my_id() {
            let game_status = match alt_game.status() {
                BughouseGameStatus::Active => {
                    return Err(internal_error!("Cannot process game over: game not over"));
                },
                BughouseGameStatus::Victory(team, _) => {
                    if team == my_player_id.team() {
                        SubjectiveGameResult::Victory
                    } else {
                        SubjectiveGameResult::Defeat
                    }
                },
                BughouseGameStatus::Draw(_) => SubjectiveGameResult::Draw,
            };
            self.notable_event_queue.push_back(NotableEvent::GameOver(game_status));
            // Note. It would make more sense to send performanse stats on leave, but there doesn't
            // seem to be a way to do this reliably, especially on mobile.
            self.report_performance();
        }
        Ok(())
    }

    fn update_scores(&mut self, new_scores: Scores) -> Result<(), EventError> {
        let contest = self.contest_mut().ok_or_else(|| internal_error!("Cannot update scores: no contest in progress"))?;
        contest.scores = new_scores;
        Ok(())
    }

    fn update_chalk_board<F>(&mut self, display_board: DisplayBoard, f: F)
    where
        F: FnOnce(&mut Chalkboard, String, BughouseBoard)
    {
        let Some(contest) = self.contest_mut() else {
            return;
        };
        let Some(ref mut game_state) = contest.game_state else {
            return;
        };
        if game_state.alt_game.status() == BughouseGameStatus::Active {
            return;
        }
        let board_idx = get_board_index(display_board, game_state.alt_game.perspective());
        f(&mut game_state.chalkboard, contest.my_name.clone(), board_idx);
        self.send_chalk_drawing_update();
    }

    fn send_chalk_drawing_update(&mut self) {
        // Caller must ensure that contest and game exist.
        let contest = self.contest().unwrap();
        let game_state = contest.game_state.as_ref().unwrap();
        let drawing = game_state.chalkboard.drawings_by(&contest.my_name).cloned()
            .unwrap_or_else(|| ChalkDrawing::new());
        self.connection.send(BughouseClientEvent::UpdateChalkDrawing{ drawing })
    }

    fn check_connection(&mut self) {
        use ActiveConnectionStatus::*;
        let now = Instant::now();
        match self.connection.health_monitor.update(now) {
            Noop => {},
            SendPing => { self.connection.send(BughouseClientEvent::Ping); },
        }
    }

    fn update_low_time_warnings(&mut self, generate_notable_events: bool) {
        let Some(game_state) = self.game_state_mut() else {
            return;
        };
        let GameState{ ref alt_game, time_pair, ref mut next_low_time_warning_idx, .. } = game_state;
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
        let mut num_events = 0;
        for board_idx in BughouseBoard::iter() {
            let Some(time_left) = my_time_left(alt_game, board_idx, game_now) else {
                return;
            };
            let idx = &mut next_low_time_warning_idx[board_idx];
            while *idx < LOW_TIME_WARNING_THRESHOLDS.len() && time_left <= LOW_TIME_WARNING_THRESHOLDS[*idx] {
                *idx += 1;
                num_events += 1;
            }
        }
        if generate_notable_events {
            for _ in 0..num_events {
                self.notable_event_queue.push_back(NotableEvent::LowTime);
            }
        }
    }
}

fn reserve_size(alt_game: &AlteredGame, envoy: BughouseEnvoy) -> Option<u8> {
    // For detecting if new reserve pieces have arrived.
    // Look at `game_confirmed`, not `local_game`. The latter would give a false positive if
    // a drop premove gets cancelled because the square is now occupied by an opponent's piece.
    Some(alt_game.game_confirmed().reserve(envoy).values().sum())
}

fn my_time_left(alt_game: &AlteredGame, board_idx: BughouseBoard, now: GameInstant) -> Option<Duration> {
    alt_game.my_id().envoy_for(board_idx).map(|e| {
        alt_game.local_game().board(e.board_idx).clock().time_left(e.force, now)
    })
}
