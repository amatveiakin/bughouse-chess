use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use instant::Instant;

use crate::altered_game::AlteredGame;
use crate::board::{TurnError, TurnMode, TurnInput};
use crate::clock::{GameInstant, WallGameTimePair};
use crate::force::Force;
use crate::game::{TurnRecord, BughouseParticipantId, BughouseObserserId, BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::event::{BughouseServerEvent, BughouseClientEvent, BughouseClientPerformance};
use crate::heartbeat::{Heart, HeartbeatOutcome};
use crate::meter::{Meter, MeterBox, MeterStats};
use crate::pgn::BughouseExportFormat;
use crate::player::{Player, Team};
use crate::rules::{ChessRules, BughouseRules};
use crate::scores::Scores;


#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TurnCommandError {
    IllegalTurn(TurnError),
    NoGameInProgress,  // TODO: Consider collapsing this into TurnError
}

#[derive(Clone, Copy, Debug)]
pub enum SubjectiveGameResult {
    Victory,
    Defeat,
    Draw,
}

#[derive(Clone, Debug)]
pub enum NotableEvent {
    GotContestId(String),
    GameStarted,
    GameOver(SubjectiveGameResult),
    MyTurnMade,
    OpponentTurnMade,
    MyReserveRestocked,
    LowTime,
    GameExportReady(String),
}

// TODO: Does it make sense to have CannotApplyEvent instead of panic? Both can be caused by many
//   invariant violations in case of bad server behavior anyway.
#[derive(Clone, Debug)]
pub enum EventError {
    ServerReturnedError(String),
    CannotApplyEvent(String),
}

#[derive(Debug)]
pub struct GameState {
    // Game state including unconfirmed local changes.
    pub alt_game: AlteredGame,
    // Game start time: `None` before first move, non-`None` afterwards.
    pub time_pair: Option<WallGameTimePair>,
    // Index of the next warning in `LOW_TIME_WARNING_THRESHOLDS`.
    next_low_time_warning_idx: usize,
    // Used to track how long it took the server to confirm a turn.
    awaiting_turn_confirmation_since: Option<Instant>,
}

#[derive(Debug)]
pub struct Contest {
    pub contest_id: String,
    pub my_name: String,
    pub my_team: Option<Team>,
    // Rules applied in every game of the contest.
    pub chess_rules: ChessRules,
    pub bughouse_rules: BughouseRules,
    // All players including those not participating in the current game.
    pub players: Vec<Player>,
    // Scores from the past matches.
    pub scores: Scores,
    // Whether this client is ready to start a new game.
    pub is_ready: bool,
    // Active game or latest game
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
    heart: Heart,
}

impl Connection {
    fn new(events_tx: mpsc::Sender<BughouseClientEvent>) -> Self {
        let now = Instant::now();
        Connection {
            events_tx,
            heart: Heart::new(now),
        }
    }

    fn send(&mut self, event: BughouseClientEvent) {
        // Improvement potential: Propagate `now` from the caller.
        let now = Instant::now();
        self.events_tx.send(event).unwrap();
        self.heart.register_outgoing(now);
    }
}

pub struct ClientState {
    user_agent: String,
    time_zone: String,
    connection: Connection,
    contest_state: ContestState,
    notable_event_queue: VecDeque<NotableEvent>,
    meter_box: MeterBox,
    turn_confirmed_meter: Meter,
}

const LOW_TIME_WARNING_THRESHOLDS: &[Duration] = &[
    Duration::from_secs(20),
    Duration::from_secs(10),
    Duration::from_secs(5),
    Duration::from_secs(3),
    Duration::from_secs(2),
    Duration::from_secs(1),
];

macro_rules! cannot_apply_event {
    ($($arg:tt)*) => {
        EventError::CannotApplyEvent(format!($($arg)*))
    }
}

impl ClientState {
    pub fn new(
        user_agent: String, time_zone: String, events_tx: mpsc::Sender<BughouseClientEvent>
    ) -> Self {
        let mut meter_box = MeterBox::new();
        let turn_confirmed_meter = meter_box.meter("turn_confirmation".to_owned());
        ClientState {
            user_agent,
            time_zone,
            connection: Connection::new(events_tx),
            contest_state: ContestState::NotConnected,
            notable_event_queue: VecDeque::new(),
            meter_box,
            turn_confirmed_meter,
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
    pub fn alt_game_mut(&mut self) -> Option<&mut AlteredGame> { self.game_state_mut().map(|c| &mut c.alt_game) }

    pub fn meter(&mut self, name: String) -> Meter { self.meter_box.meter(name) }
    pub fn read_meter_stats(&self) -> HashMap<String, MeterStats> { self.meter_box.read_stats() }
    pub fn consume_meter_stats(&mut self) -> HashMap<String, MeterStats> { self.meter_box.consume_stats() }

    pub fn is_connection_ok(&self) -> bool { self.connection.heart.healthy() }

    pub fn new_contest(&mut self, chess_rules: ChessRules, bughouse_rules: BughouseRules, my_name: String) {
        self.connection.send(BughouseClientEvent::NewContest {
            chess_rules,
            bughouse_rules,
            player_name: my_name.clone(),
        });
        self.contest_state = ContestState::Creating{ my_name };
    }
    pub fn join(&mut self, contest_id: String, my_name: String) {
        self.connection.send(BughouseClientEvent::Join {
            contest_id: contest_id.clone(),
            player_name: my_name.clone(),
        });
        self.contest_state = ContestState::Joining{ contest_id, my_name };
    }
    pub fn set_team(&mut self, team: Team) {
        if let Some(contest) = self.contest_mut() {
            contest.my_team = Some(team);
            self.connection.send(BughouseClientEvent::SetTeam{ team });
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

    pub fn make_turn(&mut self, turn_input: TurnInput) -> Result<(), TurnCommandError> {
        let game_state = self.game_state_mut().ok_or(TurnCommandError::NoGameInProgress)?;
        let GameState{ ref mut alt_game, time_pair, ref mut awaiting_turn_confirmation_since, .. } = game_state;
        let now = Instant::now();
        let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
        if alt_game.status() != BughouseGameStatus::Active {
            Err(TurnCommandError::IllegalTurn(TurnError::GameOver))
        } else if alt_game.can_make_local_turn() {
            let mode = alt_game.try_local_turn(&turn_input, game_now).map_err(|err| {
                TurnCommandError::IllegalTurn(err)
            })?;
            if mode == TurnMode::Normal {
                *awaiting_turn_confirmation_since = Some(now);
            }
            self.connection.send(BughouseClientEvent::MakeTurn{ turn_input });
            self.notable_event_queue.push_back(NotableEvent::MyTurnMade);
            Ok(())
        } else {
            Err(TurnCommandError::IllegalTurn(TurnError::WrongTurnOrder))
        }
    }

    pub fn cancel_preturn(&mut self) {
        if let Some(alt_game) = self.alt_game_mut() {
            if alt_game.cancel_preturn() {
                self.connection.send(BughouseClientEvent::CancelPreturn);
            }
        }
    }

    pub fn process_server_event(&mut self, event: BughouseServerEvent) -> Result<(), EventError> {
        use BughouseServerEvent::*;
        let now = Instant::now();
        self.connection.heart.register_incoming(now);
        match event {
            Error{ message } => {
                return Err(EventError::ServerReturnedError(format!("Got error from server: {}", message)))
            },
            ContestWelcome{ contest_id, chess_rules, bughouse_rules } => {
                let my_name = match &self.contest_state {
                    ContestState::Creating{ my_name } => {
                        self.notable_event_queue.push_back(NotableEvent::GotContestId(contest_id.clone()));
                        my_name.clone()
                    }
                    ContestState::Joining{ contest_id: id, my_name } => {
                        if contest_id != *id {
                            return Err(cannot_apply_event!("Cannot apply ContestWelcome: expected contest {id}, but got {contest_id}"));
                        }
                        my_name.clone()
                    },
                    _ => return Err(cannot_apply_event!("Cannot apply ContestWelcome: not expecting a new contest")),
                };
                self.contest_state = ContestState::Connected(Contest {
                    contest_id,
                    my_name,
                    my_team: None,
                    chess_rules,
                    bughouse_rules,
                    players: Vec::new(),
                    scores: Scores::new(),
                    is_ready: false,
                    game_state: None,
                });
            },
            LobbyUpdated{ players } => {
                let contest = self.contest_mut().ok_or_else(|| cannot_apply_event!("Cannot apply LobbyUpdated: no contest in progress"))?;
                // TODO: Fix race condition: is_ready will toggle back and forth if a lobby update
                //   (e.g. is_ready from another player) arrived before is_ready update from this
                //   client reached the server. Same for `my_team`.
                let me = players.iter().find(|p| p.name == contest.my_name).unwrap();
                contest.is_ready = me.is_ready;
                contest.my_team = me.fixed_team;
                contest.players = players;
            },
            GameStarted{ starting_position, players, time, turn_log, game_status, scores } => {
                let player_map = BughouseGame::make_player_map(
                    players.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                );
                let time_pair = if turn_log.is_empty() {
                    assert!(time.elapsed_since_start().is_zero());
                    None
                } else {
                    Some(WallGameTimePair::new(now, time.approximate()))
                };
                let contest = self.contest_mut().ok_or_else(|| cannot_apply_event!("Cannot apply GameStarted: no contest in progress"))?;
                let game = BughouseGame::new_with_starting_position(
                    contest.chess_rules.clone(), contest.bughouse_rules.clone(), starting_position, player_map
                );
                let my_id = match game.find_player(&contest.my_name) {
                    Some(id) => BughouseParticipantId::Player(id),
                    None => BughouseParticipantId::Observer(BughouseObserserId {
                        board_idx: BughouseBoard::A,
                        force: Force::White,
                    }),
                };
                let alt_game = AlteredGame::new(my_id, game);
                contest.game_state = Some(GameState {
                    alt_game,
                    time_pair,
                    next_low_time_warning_idx: 0,
                    awaiting_turn_confirmation_since: None,
                });
                for turn in turn_log {
                    self.apply_remote_turn(turn, false)?;
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
                let contest = self.contest_mut().ok_or_else(|| cannot_apply_event!("Cannot apply GameOver: no contest in progress"))?;
                let game_state = contest.game_state.as_mut().ok_or_else(|| cannot_apply_event!("Cannot apply GameOver: no game in progress"))?;
                assert!(game_state.alt_game.status() == BughouseGameStatus::Active);
                game_state.alt_game.set_status(game_status, time);
                contest.scores = new_scores;
                self.game_over_postprocess()?;
            },
            GameExportReady{ content } => {
                self.notable_event_queue.push_back(NotableEvent::GameExportReady(content));
            },
            Heartbeat => {
                // This event is needed only for `heart.register_incoming` above.
            }
        }
        Ok(())
    }

    pub fn next_notable_event(&mut self) -> Option<NotableEvent> {
        self.notable_event_queue.pop_front()
    }

    fn apply_remote_turn(&mut self, turn_record: TurnRecord, generate_notable_events: bool)
        -> Result<(), EventError>
    {
        let TurnRecord{ player_id, turn_algebraic, time } = turn_record;
        let ContestState::Connected(contest) = &mut self.contest_state else {
            return Err(cannot_apply_event!("Cannot make turn: no contest in progress"));
        };
        let game_state = contest.game_state.as_mut().ok_or_else(|| cannot_apply_event!("Cannot make turn: no game in progress"))?;
        let GameState{ ref mut alt_game, ref mut time_pair, ref mut awaiting_turn_confirmation_since, .. } = game_state;
        if alt_game.status() != BughouseGameStatus::Active {
            return Err(cannot_apply_event!("Cannot make turn {}: game over", turn_algebraic));
        }
        let now = Instant::now();
        if let BughouseParticipantId::Player(my_player_id) = alt_game.my_id() {
            if player_id == my_player_id {
                // It's normal that the client is not awaiting confirmation, because preturns are not confirmed.
                if let Some(t0) = awaiting_turn_confirmation_since {
                    let d = now.duration_since(*t0);
                    self.turn_confirmed_meter.record_duration(d);
                    *awaiting_turn_confirmation_since = None;
                }
            }
        }
        if time_pair.is_none() {
            // Improvement potential. Sync client/server times better; consider NTP.
            let game_start = GameInstant::game_start().approximate();
            *time_pair = Some(WallGameTimePair::new(now, game_start));
        }
        let old_reserve_size = my_reserve_size(alt_game);
        alt_game.apply_remote_turn_algebraic(
            player_id, &turn_algebraic, time
        ).map_err(|err| {
            cannot_apply_event!("Impossible turn: {}, error: {:?}", turn_algebraic, err)
        })?;
        let new_reserve_size = my_reserve_size(alt_game);
        if generate_notable_events {
            if let BughouseParticipantId::Player(my_player_id) = alt_game.my_id() {
                if player_id == my_player_id.opponent() {
                    self.notable_event_queue.push_back(NotableEvent::OpponentTurnMade);
                }
            }
            if new_reserve_size > old_reserve_size {
                self.notable_event_queue.push_back(NotableEvent::MyReserveRestocked);
            }
        }
        Ok(())
    }

    fn verify_game_status(&mut self, game_status: BughouseGameStatus) -> Result<(), EventError> {
        let contest = self.contest_mut().ok_or_else(|| cannot_apply_event!("Cannot verify game status: no contest in progress"))?;
        let game_state = contest.game_state.as_mut().ok_or_else(|| cannot_apply_event!("Cannot verify game status: no game in progress"))?;
        let GameState{ ref mut alt_game, .. } = game_state;
        if game_status != alt_game.status() {
            return Err(cannot_apply_event!(
                "Expected game status = {:?}, actual = {:?}", game_status, alt_game.status()
            ));
        }
        Ok(())
    }

    fn update_game_status(&mut self, game_status: BughouseGameStatus, game_now: GameInstant)
        -> Result<(), EventError>
    {
        let contest = self.contest_mut().ok_or_else(|| cannot_apply_event!("Cannot update game status: no contest in progress"))?;
        let game_state = contest.game_state.as_mut().ok_or_else(|| cannot_apply_event!("Cannot update game status: no game in progress"))?;
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
        let contest = self.contest_mut().ok_or_else(|| cannot_apply_event!("Cannot process game over: no contest in progress"))?;
        let game_state = contest.game_state.as_mut().ok_or_else(|| cannot_apply_event!("Cannot process game over: no game in progress"))?;
        let GameState{ ref mut alt_game, .. } = game_state;
        if let BughouseParticipantId::Player(my_player_id) = alt_game.my_id() {
            let game_status = match alt_game.status() {
                BughouseGameStatus::Active => {
                    return Err(cannot_apply_event!("Cannot process game over: game not over"));
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
        let contest = self.contest_mut().ok_or_else(|| cannot_apply_event!("Cannot update scores: no contest in progress"))?;
        contest.scores = new_scores;
        Ok(())
    }

    fn check_connection(&mut self) {
        use HeartbeatOutcome::*;
        let now = Instant::now();
        match self.connection.heart.beat(now) {
            AllGood => {},
            SendBeat => {
                self.connection.send(BughouseClientEvent::Heartbeat);
            },
            OtherPartyTemporatyLost => {},
            OtherPartyPermanentlyLost => {},
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
        let Some(time_left) = my_time_left(alt_game, game_now) else {
            return;
        };
        let idx = next_low_time_warning_idx;
        let mut num_events = 0;
        while *idx < LOW_TIME_WARNING_THRESHOLDS.len() && time_left <= LOW_TIME_WARNING_THRESHOLDS[*idx] {
            *idx += 1;
            num_events += 1;
        }
        if generate_notable_events {
            for _ in 0..num_events {
                self.notable_event_queue.push_back(NotableEvent::LowTime);
            }
        }
    }
}

fn my_reserve_size(alt_game: &AlteredGame) -> Option<u8> {
    // For detecting if new reserve pieces have arrived.
    // Look at `game_confirmed`, not `local_game`. The latter would give a false positive if
    // a drop premove gets cancelled because the square is now occupied by an opponent's piece.
    if let BughouseParticipantId::Player(my_player_id) = alt_game.my_id() {
        Some(alt_game.game_confirmed().reserve(my_player_id).values().sum())
    } else {
        None
    }
}

fn my_time_left(alt_game: &AlteredGame, now: GameInstant) -> Option<Duration> {
    if let BughouseParticipantId::Player(my_player_id) = alt_game.my_id() {
        Some(alt_game.local_game().board(my_player_id.board_idx).clock().time_left(my_player_id.force, now))
    } else {
        None
    }
}
