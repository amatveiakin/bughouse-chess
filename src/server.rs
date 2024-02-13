// Improvement potential. Replace `game.find_player(&self.players[participant_id].name)`
//   with a direct mapping (participant_id -> player_bughouse_id).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use std::{iter, mem, ops};

use enum_map::enum_map;
use indoc::printdoc;
use instant::Instant;
use itertools::Itertools;
use lazy_static::lazy_static;
use log::{info, warn};
use prometheus::{register_histogram_vec, HistogramVec};
use rand::prelude::*;
use strum::IntoEnumIterator;

use crate::board::{TurnInput, TurnMode, VictoryReason};
use crate::chalk::{ChalkDrawing, Chalkboard};
use crate::chat::{ChatMessage, ChatMessageBody, ChatRecipient, OutgoingChatMessage};
use crate::clock::GameInstant;
use crate::event::{
    BughouseClientErrorReport, BughouseClientEvent, BughouseServerEvent, BughouseServerRejection,
    GameUpdate,
};
use crate::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus, BughousePlayer, PlayerInGame,
    TurnRecord,
};
use crate::half_integer::HalfU32;
use crate::iterable_mut::IterableMut;
use crate::lobby::{assign_boards, fix_teams_if_needed, verify_participants, Teaming};
use crate::pgn::{self, BughouseExportFormat};
use crate::ping_pong::{PassiveConnectionMonitor, PassiveConnectionStatus};
use crate::player::{Faction, Participant};
use crate::role::Role;
use crate::rules::{Rules, FIRST_GAME_COUNTDOWN_DURATION};
use crate::scores::Scores;
use crate::server_chat::{ChatRecipientExpanded, ServerChat};
use crate::server_helpers::ServerHelpers;
use crate::server_hooks::{NoopServerHooks, ServerHooks};
use crate::session::Session;
use crate::session_store::{SessionId, SessionStore};
use crate::utc_time::UtcDateTime;
use crate::util::Relax;
use crate::{fetch_new_chat_messages, my_git_version};


const DOUBLE_TERMINATION_ABORT_THRESHOLD: Duration = Duration::from_secs(1);
const TERMINATION_WAITING_PERIOD: Duration = Duration::from_secs(60);
const MATCH_GC_INACTIVITY_THRESHOLD: Duration = Duration::from_secs(3600 * 24);

lazy_static! {
    static ref EVENT_PROCESSING_HISTOGRAM: HistogramVec = register_histogram_vec!(
        "event_processing_time_seconds",
        "Incoming event processing time in seconds.",
        &["event"],
        vec![0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1],
    )
    .unwrap();
}

macro_rules! unknown_error {
    ($($arg:tt)*) => {
        BughouseServerRejection::UnknownError{ message: $crate::internal_error_message!($($arg)*) }
    }
}

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub check_git_version: bool,
    pub max_starting_time: Option<Duration>,
}

#[derive(Clone, Copy, Debug)]
enum Execution {
    // The server runs normally.
    Running,

    // The server is in graceful shutdown mode. It will not allow to start new matches or
    // new games within existing matches and it will automatically shut down when there are
    // no more games running.
    ShuttingDown {
        // The moment shutdown was requested initially.
        shutting_down_since: Instant,
        // Last termination request (i.e. last time Ctrl+C was pressed). The server will abort
        // upon two termination requests come within `DOUBLE_TERMINATION_ABORT_THRESHOLD` period.
        last_termination_request: Instant,
    },
}

#[derive(Debug)]
pub enum IncomingEvent {
    Network(ClientId, BughouseClientEvent),
    Tick,
    Terminate,
}

// TODO: Use Prometheus instead.
#[derive(Clone, Debug, Default)]
pub struct ServerInfo {
    pub num_active_matches: usize,
}

impl ServerInfo {
    pub fn new() -> Self { ServerInfo::default() }
}

#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub envoy: BughouseEnvoy,
    pub turn_input: TurnInput,
}

#[derive(Debug)]
pub struct GameState {
    // The index of this game within the match.
    game_index: u64,
    game: BughouseGame,
    // `game_creation` is the time when seating and starting position were
    // generated and presented to the users.
    game_creation: Instant,
    // `game_start` is the time when the clock started after a player made their
    // first turn. We need both an Instant and an OffsetDateTime: the instant
    // time is used for monotonic in-game time tracking, and the offset time is
    // used for communication with outside world about absolute moments in time.
    game_start: Option<Instant>,
    game_start_offset_time: Option<time::OffsetDateTime>,
    game_end: Option<Instant>,
    // All game updates. Mostly duplicates turn log. Used for reconnection.
    updates: Vec<GameUpdate>,
    // Turns requests by clients that have been executed yet. Presumably because
    // these are preturns, but maybe we'll have other reasons in the future, e.g.
    // attemping to drop an as-of-yet-missing piece.
    turn_requests: Vec<TurnRequest>,
    chalkboard: Chalkboard,
}

impl GameState {
    pub fn game(&self) -> &BughouseGame { &self.game }
    pub fn start_offset_time(&self) -> Option<time::OffsetDateTime> { self.game_start_offset_time }
}


#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct MatchId(String);


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
struct ParticipantId(usize);

#[derive(Clone, Debug)]
struct ParticipantExtra {
    confirmed_local_message_id: u64,
}

#[derive(Debug)]
struct Participants {
    // Use an ordered map to show lobby players in joining order.
    map: BTreeMap<ParticipantId, (Participant, ParticipantExtra)>,
    next_id: usize,
}

impl Participants {
    fn new() -> Self { Self { map: BTreeMap::new(), next_id: 1 } }
    fn iter(&self) -> impl Iterator<Item = &Participant> + Clone {
        self.map.values().map(|(p, _)| p)
    }
    fn iter_mut(&mut self) -> impl Iterator<Item = &mut Participant> {
        self.map.values_mut().map(|(p, _)| p)
    }
    fn extra(&self, id: ParticipantId) -> &ParticipantExtra { &self.map[&id].1 }
    fn extra_mut(&mut self, id: ParticipantId) -> &mut ParticipantExtra {
        &mut self.map.get_mut(&id).unwrap().1
    }
    fn find_by_name(&self, name: &str) -> Option<ParticipantId> {
        self.map
            .iter()
            .find_map(|(id, (p, _))| if p.name == name { Some(*id) } else { None })
    }
    fn add_participant(&mut self, participant: Participant) -> ParticipantId {
        let id = ParticipantId(self.next_id);
        self.next_id += 1;
        let extra = ParticipantExtra { confirmed_local_message_id: 0 };
        assert!(self.map.insert(id, (participant, extra)).is_none());
        id
    }
}

impl ops::Index<ParticipantId> for Participants {
    type Output = Participant;
    fn index(&self, id: ParticipantId) -> &Self::Output { &self.map[&id].0 }
}
impl ops::IndexMut<ParticipantId> for Participants {
    fn index_mut(&mut self, id: ParticipantId) -> &mut Self::Output {
        &mut self.map.get_mut(&id).unwrap().0
    }
}

impl IterableMut<Participant> for Participants {
    fn get_iter(&self) -> impl Iterator<Item = &Participant> { self.iter() }
    fn get_iter_mut(&mut self) -> impl Iterator<Item = &mut Participant> { self.iter_mut() }
}


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClientId(usize);

#[derive(Debug)]
pub struct Client {
    events_tx: async_std::channel::Sender<BughouseServerEvent>,
    new_client: bool,
    match_id: Option<MatchId>,
    participant_id: Option<ParticipantId>,
    session_id: Option<SessionId>,
    logging_id: String,
    connection_monitor: PassiveConnectionMonitor,
}

impl Client {
    fn send(&mut self, event: BughouseServerEvent) { self.events_tx.try_send(event).unwrap(); }
    fn send_rejection(&mut self, rejection: BughouseServerRejection) {
        self.send(BughouseServerEvent::Rejection(rejection));
    }
}

#[derive(Debug)]
pub struct Clients {
    map: HashMap<ClientId, Client>,
    next_id: usize,
}

impl Clients {
    pub fn new() -> Self { Clients { map: HashMap::new(), next_id: 1 } }

    pub fn add_client(
        &mut self, events_tx: async_std::channel::Sender<BughouseServerEvent>,
        session_id: Option<SessionId>, logging_id: String,
    ) -> ClientId {
        let now = Instant::now();
        let client = Client {
            events_tx,
            new_client: true,
            match_id: None,
            participant_id: None,
            session_id,
            logging_id,
            connection_monitor: PassiveConnectionMonitor::new(now),
        };
        let id = ClientId(self.next_id);
        self.next_id += 1;
        assert!(self.map.insert(id, client).is_none());
        id
    }

    // Returns `logging_id` if the client existed.
    // A client can be removed multiple times, e.g. first on `Leave`, then on network
    // channel closure. This is not an error.
    //
    // TODO: Make sure network connection is closed in a reasnable timeframe whenever
    //   a client is removed.
    //
    // Improvement potential. Send an event informing other clients that somebody went
    // offline (for TUI: could use “ϟ” for “disconnected”; there is a plug emoji “🔌”
    // that works much better, but it's not supported by console fonts).
    pub fn remove_client(&mut self, id: ClientId) -> Option<String> {
        self.map.remove(&id).map(|client| client.logging_id)
    }

    // Sends the event to each client who has joined the match.
    //
    // Improvement potential. Do not iterate over all clients. Keep the list of clients
    // in each match.
    fn broadcast(&mut self, match_id: &MatchId, event: &BughouseServerEvent) {
        for client in self.map.values_mut() {
            if client.match_id.as_ref() == Some(match_id) {
                client.send(event.clone());
            }
        }
    }

    fn welcome_new_clients(&mut self, event: &BughouseServerEvent) {
        for client in self.map.values_mut() {
            if client.new_client {
                client.send(event.clone());
                client.new_client = false;
            }
        }
    }

    fn find_participant(
        &self, match_id: &MatchId, participant_id: ParticipantId,
    ) -> Option<ClientId> {
        self.map
            .iter()
            .find(|(_, c)| {
                c.match_id.as_ref() == Some(match_id) && c.participant_id == Some(participant_id)
            })
            .map(|(&id, _)| id)
    }
}

impl ops::Index<ClientId> for Clients {
    type Output = Client;
    fn index(&self, id: ClientId) -> &Self::Output { &self.map[&id] }
}
impl ops::IndexMut<ClientId> for Clients {
    fn index_mut(&mut self, id: ClientId) -> &mut Self::Output { self.map.get_mut(&id).unwrap() }
}


#[derive(Clone, PartialEq, Eq, Debug)]
enum MatchActivity {
    Present,
    Past(Instant),
}

#[derive(Debug)]
struct Match {
    match_id: MatchId,
    match_creation: Instant,
    rules: Rules,
    participants: Participants,
    chat: ServerChat,
    teaming: Option<Teaming>,         // `Some` since the first game begins
    scores: Option<Scores>,           // `Some` since the first game begins
    match_history: Vec<BughouseGame>, // final game states; TODO: rename to `game history`
    first_game_countdown_since: Option<Instant>,
    next_board_assignment: Option<Vec<PlayerInGame>>,
    game_state: Option<GameState>, // active game or latest game
}

struct Context<'a, 'b> {
    // Use the same timestamps for the entire event processing. Code that has access to `Context`
    // should not call `Instant::now()` or `UtcDateTime::now()`. Doing so may cause a race
    // condition. For example: at t1 we check the flag and see that it's ok; at t2 (t2 > t1) we
    // accept a turn, despite the fact that the game should've been over by t2.
    now: Instant,
    utc_now: UtcDateTime,

    clients: &'b mut MutexGuard<'a, Clients>,
    session_store: &'b mut MutexGuard<'a, SessionStore>,
    info: &'b mut MutexGuard<'a, ServerInfo>,
    helpers: &'a mut dyn ServerHelpers,
    hooks: &'a mut dyn ServerHooks,

    disable_countdown: bool,
    disable_connection_health_check: bool,
}

struct CoreServerState {
    server_options: ServerOptions,
    welcome_event: BughouseServerEvent,
    execution: Execution,
    matches: HashMap<MatchId, Match>,
}

type EventResult = Result<(), BughouseServerRejection>;

// Split state into two parts (core and context) in order to allow things like:
//   let mut clients = self.clients.lock().unwrap();
//   self.core.foo(&mut clients);
// whereas
//   self.foo(&mut clients);
// would make the compiler complain that `self` is borrowed twice.
pub struct ServerState {
    // Optimization potential: Lock-free map instead of Mutex<HashMap>.
    clients: Arc<Mutex<Clients>>,
    session_store: Arc<Mutex<SessionStore>>,
    info: Arc<Mutex<ServerInfo>>,
    helpers: Box<dyn ServerHelpers>,
    hooks: Box<dyn ServerHooks>,
    // TODO: Remove special test paths, use proper mocks instead.
    disable_countdown: bool,               // for tests
    disable_connection_health_check: bool, // for tests
    core: CoreServerState,
}

impl ServerState {
    pub fn new(
        server_options: ServerOptions, clients: Arc<Mutex<Clients>>,
        session_store: Arc<Mutex<SessionStore>>, info: Arc<Mutex<ServerInfo>>,
        helpers: Box<dyn ServerHelpers>, hooks: Option<Box<dyn ServerHooks>>,
    ) -> Self {
        ServerState {
            clients,
            session_store,
            info,
            helpers,
            hooks: hooks.unwrap_or_else(|| Box::new(NoopServerHooks {})),
            disable_countdown: false,
            disable_connection_health_check: false,
            core: CoreServerState::new(server_options),
        }
    }

    pub fn apply_event(&mut self, event: IncomingEvent, now: Instant, utc_now: UtcDateTime) {
        // Lock clients for the entire duration of the function. This means simpler and
        // more predictable event processing, e.g. it gives a guarantee that all broadcasts
        // from a single `apply_event` reach the same set of clients.
        // Similar for session_store.
        //
        // TODO: Rethink this approach. This is almost a GIL. It makes the server de facto
        // single-threaded.
        let mut clients = self.clients.lock().unwrap();
        let mut session_store = self.session_store.lock().unwrap();
        let mut info = self.info.lock().unwrap();

        // Improvement potential. Consider adding commonly used things like `now` and `execution`
        // to `Context`.
        let mut ctx = Context {
            now,
            utc_now,
            clients: &mut clients,
            session_store: &mut session_store,
            info: &mut info,
            helpers: self.helpers.as_mut(),
            hooks: self.hooks.as_mut(),
            disable_countdown: self.disable_countdown,
            disable_connection_health_check: self.disable_connection_health_check,
        };

        self.core.apply_event(&mut ctx, event);
    }

    #[allow(non_snake_case)]
    pub fn TEST_disable_countdown(&mut self) { self.disable_countdown = true; }

    #[allow(non_snake_case)]
    pub fn TEST_disable_connection_health_check(&mut self) {
        self.disable_connection_health_check = true;
    }

    #[allow(non_snake_case)]
    pub fn TEST_override_board_assignment(
        &mut self, match_id: String, assignment: Vec<PlayerInGame>,
    ) {
        let match_id = MatchId(match_id);
        let mtch = self.core.matches.get_mut(&match_id).unwrap();
        for player_assignment in &assignment {
            if let Some(player) =
                mtch.participants.iter().find(|p| p.name == player_assignment.name)
            {
                if let Faction::Fixed(team) = player.faction {
                    assert_eq!(team, player_assignment.id.team());
                }
            }
        }
        mtch.next_board_assignment = Some(assignment);
    }
}

impl CoreServerState {
    fn new(server_options: ServerOptions) -> Self {
        let welcome_event = BughouseServerEvent::ServerWelcome {
            expected_git_version: server_options
                .check_git_version
                .then(|| my_git_version!().to_owned()),
            max_starting_time: server_options.max_starting_time,
        };
        CoreServerState {
            server_options,
            welcome_event,
            execution: Execution::Running,
            matches: HashMap::new(),
        }
    }

    fn make_match(
        &mut self, now: Instant, rules: Rules,
    ) -> Result<MatchId, BughouseServerRejection> {
        // Client should verify rules according to the very same logic, so this shouldn't happen:
        rules.verify().map_err(|err| unknown_error!("Invalid match rules: {err}"))?;

        if let Some(max_starting_time) = self.server_options.max_starting_time {
            let starting_time = rules.chess_rules.time_control.starting_time;
            if starting_time > max_starting_time {
                // TODO: Log to see if this is a popular request.
                return Err(BughouseServerRejection::MaxStartingTimeExceeded {
                    requested: rules.chess_rules.time_control.starting_time,
                    allowed: max_starting_time,
                });
            }
        }

        // Exclude confusing characters:
        //   - 'O' and '0' (easy to confuse);
        //   - 'I' (looks like '1'; keep '1' because confusion in the other direction seems less likely).
        const ALPHABET: [char; 33] = [
            '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H',
            'J', 'K', 'L', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
        ];
        const MIN_ID_LEN: usize = 4;
        const MAX_ATTEMPTS_PER_LEN: usize = 100;
        let mut rng = rand::thread_rng();
        let mut id_len = MIN_ID_LEN;
        let mut id = MatchId(String::new());
        let mut attempts_at_this_len = 0;
        while id.0.is_empty() || self.matches.contains_key(&id) {
            id = MatchId(
                (&mut rng)
                    .sample_iter(rand::distributions::Uniform::from(0..ALPHABET.len()))
                    .map(|idx| ALPHABET[idx])
                    .take(id_len)
                    .collect(),
            );
            attempts_at_this_len += 1;
            if attempts_at_this_len > MAX_ATTEMPTS_PER_LEN {
                id_len += 1;
                attempts_at_this_len = 0;
            }
        }
        // TODO: Verify that time limit is not too large: the match will not be GCed while the
        //   clock's ticking even if all players left.
        let mtch = Match {
            match_id: id.clone(),
            match_creation: now,
            rules,
            participants: Participants::new(),
            chat: ServerChat::new(),
            teaming: None,
            scores: None,
            match_history: Vec::new(),
            first_game_countdown_since: None,
            next_board_assignment: None,
            game_state: None,
        };
        assert!(self.matches.insert(id.clone(), mtch).is_none());
        Ok(id)
    }

    fn apply_event(&mut self, ctx: &mut Context, event: IncomingEvent) {
        let timer = EVENT_PROCESSING_HISTOGRAM
            .with_label_values(&[event_name(&event)])
            .start_timer();

        ctx.clients.welcome_new_clients(&self.welcome_event);

        match event {
            IncomingEvent::Network(client_id, event) => self.on_client_event(ctx, client_id, event),
            IncomingEvent::Tick => self.on_tick(ctx),
            IncomingEvent::Terminate => self.on_terminate(ctx),
        }
        ctx.info.num_active_matches = self.num_active_matches(ctx.now);

        timer.observe_duration();
    }

    fn on_client_event(
        &mut self, ctx: &mut Context, client_id: ClientId, event: BughouseClientEvent,
    ) {
        if !ctx.clients.map.contains_key(&client_id) {
            // TODO: Should there be an exception for `BughouseClientEvent::ReportError`?
            // TODO: Improve logging. Consider:
            //   - include logging_id inside client_id, or
            //   - keep disconnected clients in the map for some time.
            warn!("Got an event from disconnected client:\n{event:?}");
            return;
        }

        ctx.clients[client_id].connection_monitor.register_incoming(ctx.now);

        // First, process events that don't require a match.
        match &event {
            BughouseClientEvent::ReportPerformace(perf) => {
                ctx.hooks.on_client_performance_report(perf);
                return;
            }
            BughouseClientEvent::ReportError(report) => {
                process_report_error(ctx, client_id, report);
                return;
            }
            BughouseClientEvent::Ping => {
                process_ping(ctx, client_id);
                return;
            }
            _ => {}
        };

        let match_id = match &event {
            BughouseClientEvent::NewMatch { rules, .. } => {
                if !matches!(self.execution, Execution::Running) {
                    ctx.clients[client_id].send_rejection(BughouseServerRejection::ShuttingDown);
                    return;
                }
                ctx.clients[client_id].match_id = None;
                ctx.clients[client_id].participant_id = None;
                let match_id = match self.make_match(ctx.now, rules.clone()) {
                    Ok(id) => id,
                    Err(err) => {
                        ctx.clients[client_id].send_rejection(err);
                        return;
                    }
                };
                info!(
                    "Match {} created by client {}",
                    match_id.0, ctx.clients[client_id].logging_id
                );
                Some(match_id)
            }
            BughouseClientEvent::Join { match_id, .. }
            | BughouseClientEvent::HotReconnect { match_id, .. } => {
                // Improvement potential: Log cases when a client reconnects to their current
                //   match. This likely indicates a client error.
                ctx.clients[client_id].match_id = None;
                ctx.clients[client_id].participant_id = None;
                Some(MatchId(match_id.clone()))
            }
            _ => ctx.clients[client_id].match_id.clone(),
        };

        let Some(match_id) = match_id else {
            // We've already processed all events that do not depend on a match.
            ctx.clients[client_id].send_rejection(unknown_error!());
            return;
        };

        let Some(mtch) = self.matches.get_mut(&match_id) else {
            // The only way to have a match_id with no match is when a client is trying
            // to join with a bad match_id. In other cases we are getting match_id from
            // trusted internal sources, so the match must exist as well.
            assert!(
                matches!(event, BughouseClientEvent::Join { .. })
                    || matches!(event, BughouseClientEvent::HotReconnect { .. })
            );
            ctx.clients[client_id]
                .send_rejection(BughouseServerRejection::NoSuchMatch { match_id: match_id.0 });
            return;
        };

        // Test flags first. Thus we make sure that turns and other actions are
        // not allowed after the time is over.
        mtch.test_flags(ctx);
        mtch.process_client_event(ctx, client_id, self.execution, event);
        mtch.post_process(ctx, self.execution);
    }

    fn on_tick(&mut self, ctx: &mut Context) {
        self.gc_old_matches(ctx.now);
        self.check_client_connections(ctx);
        for mtch in self.matches.values_mut() {
            mtch.test_flags(ctx);
            mtch.post_process(ctx, self.execution);
        }
        if !matches!(self.execution, Execution::Running) && self.num_active_matches(ctx.now) == 0 {
            println!("No more active matches left. Shutting down.");
            shutdown();
        }
    }

    pub fn on_terminate(&mut self, ctx: &mut Context) {
        const ABORT_INSTRUCTION: &str = "Press Ctrl+C twice within a second to abort immediately.";
        let num_active_matches = self.num_active_matches(ctx.now);
        match self.execution {
            Execution::Running => {
                if num_active_matches == 0 {
                    println!("There are no active matches. Shutting down immediately!");
                    shutdown();
                } else {
                    printdoc!("
                        Shutdown requested!
                        The server will not allow to start new matches or games. It will terminate as
                        soon as there are no active matches. There are currently {num_active_matches} active matches.
                        {ABORT_INSTRUCTION}
                    ");
                    self.execution = Execution::ShuttingDown {
                        shutting_down_since: ctx.now,
                        last_termination_request: ctx.now,
                    };
                    self.matches.values().for_each(|mtch| {
                        // Immediately notify clients who are still in the lobby: they wouldn't be
                        // able to do anything meaningful. Let other players finish their games.
                        //
                        // Improvement potential: Notify everybody immediately, let the clients
                        // decide when it's appropriate to show the message to the user. Pros:
                        //   - Server code will be simpler. There will be exactly two points when a
                        //     shutdown notice should be sent: to all existing clients when
                        //     termination is requested and to new clients as soon as they are
                        //     connected (to the server, not to the match).
                        //   - Clients will get the relevant information sooner.
                        if mtch.game_state.is_none() {
                            mtch.broadcast(
                                ctx,
                                &BughouseServerEvent::Rejection(
                                    BughouseServerRejection::ShuttingDown,
                                ),
                            );
                        }
                    });
                }
            }
            Execution::ShuttingDown {
                shutting_down_since,
                ref mut last_termination_request,
            } => {
                if ctx.now.duration_since(*last_termination_request)
                    <= DOUBLE_TERMINATION_ABORT_THRESHOLD
                {
                    println!("Aborting!");
                    shutdown();
                } else {
                    let shutdown_duration_sec =
                        ctx.now.duration_since(shutting_down_since).as_secs();
                    println!(
                        "Shutdown was requested {}s ago. Waiting for {} active matches to finish.\n{}",
                        shutdown_duration_sec,
                        num_active_matches,
                        ABORT_INSTRUCTION,
                    );
                }
                *last_termination_request = ctx.now;
            }
        }
    }

    fn num_active_matches(&self, now: Instant) -> usize {
        self.matches
            .values()
            .filter(|mtch| match mtch.latest_activity() {
                MatchActivity::Present => true,
                MatchActivity::Past(t) => now.duration_since(t) <= TERMINATION_WAITING_PERIOD,
            })
            .count()
    }

    fn gc_old_matches(&mut self, now: Instant) {
        // Improvement potential. GC unused matches (zero games and/or no players) sooner.
        self.matches.retain(|_, mtch| match mtch.latest_activity() {
            MatchActivity::Present => true,
            MatchActivity::Past(t) => now.duration_since(t) <= MATCH_GC_INACTIVITY_THRESHOLD,
        });
    }

    fn check_client_connections(&mut self, ctx: &mut Context) {
        use PassiveConnectionStatus::*;
        if ctx.disable_connection_health_check {
            return;
        }
        ctx.clients
            .map
            .retain(|_, client| match client.connection_monitor.status(ctx.now) {
                Healthy | TemporaryLost => true,
                PermanentlyLost => false,
            });
    }
}

impl Match {
    fn latest_activity(&self) -> MatchActivity {
        if let Some(GameState { game_creation, game_start, game_end, .. }) = self.game_state {
            if let Some(game_end) = game_end {
                MatchActivity::Past(game_end)
            } else if game_start.is_some() {
                MatchActivity::Present
            } else {
                // Since `latest_activity` is used for things like match GC, we do not want to
                // count the period between game creation and game start as activity:
                //   - In a normal case players will start the game soon, so the match will not
                //     be GCed;
                //   - In a pathological case the match could stay in this state indefinitely,
                //     leading to an unrecoverable leak. The only safe time to consider a match to
                //     be active is when a game is when the game is active and the clock's ticking,
                //     because this period is inherently time-bound.
                MatchActivity::Past(game_creation)
            }
        } else {
            MatchActivity::Past(self.match_creation)
        }
    }

    fn test_flags(&mut self, ctx: &mut Context) {
        let Some(GameState {
            game_index,
            game_start,
            game_start_offset_time,
            ref mut game_end,
            ref mut game,
            ref mut turn_requests,
            ..
        }) = self.game_state
        else {
            return;
        };
        let Some(game_start) = game_start else {
            return;
        };
        if !game.is_active() {
            return;
        }
        let game_now = GameInstant::from_now_game_active(game_start, ctx.now);
        if let Some(game_over_time) = game.test_flag(game_now) {
            assert!(!game.is_active());
            let update = update_on_game_over(
                ctx,
                self.teaming.unwrap(),
                game_index,
                game,
                turn_requests,
                &mut self.participants,
                self.scores.as_mut().unwrap(),
                &mut self.next_board_assignment,
                &mut self.chat,
                game_over_time,
                game_start_offset_time,
                game_end,
            );
            self.add_game_updates(ctx, vec![update]);
        }
    }

    fn add_game_updates(&mut self, ctx: &mut Context, new_updates: Vec<GameUpdate>) {
        let Some(GameState { ref mut updates, .. }) = self.game_state else {
            return;
        };
        updates.extend_from_slice(&new_updates);
        let ev = BughouseServerEvent::GameUpdated { updates: new_updates };
        self.broadcast(ctx, &ev);
    }

    fn broadcast(&self, ctx: &mut Context, event: &BughouseServerEvent) {
        ctx.clients.broadcast(&self.match_id, event);
    }

    fn process_client_event(
        &mut self, ctx: &mut Context, client_id: ClientId, execution: Execution,
        event: BughouseClientEvent,
    ) {
        let result = match event {
            BughouseClientEvent::NewMatch { player_name, .. } => {
                // The match was created earlier.
                self.join_participant(ctx, client_id, execution, player_name, false)
            }
            BughouseClientEvent::Join { match_id: _, player_name } => {
                self.join_participant(ctx, client_id, execution, player_name, false)
            }
            BughouseClientEvent::HotReconnect { match_id: _, player_name } => {
                self.join_participant(ctx, client_id, execution, player_name, true)
            }
            BughouseClientEvent::SetFaction { faction } => {
                self.process_set_faction(ctx, client_id, faction)
            }
            BughouseClientEvent::SetTurns { turns } => {
                self.process_set_turns(ctx, client_id, turns)
            }
            BughouseClientEvent::MakeTurn { board_idx, turn_input } => {
                self.process_make_turn(ctx, client_id, board_idx, turn_input)
            }
            BughouseClientEvent::CancelPreturn { board_idx } => {
                self.process_cancel_preturn(ctx, client_id, board_idx)
            }
            BughouseClientEvent::Resign => self.process_resign(ctx, client_id),
            BughouseClientEvent::SetReady { is_ready } => {
                self.process_set_ready(ctx, client_id, is_ready)
            }
            BughouseClientEvent::LeaveMatch => self.process_leave_match(ctx, client_id),
            BughouseClientEvent::LeaveServer => self.process_leave_server(ctx, client_id),
            BughouseClientEvent::SendChatMessage { message } => {
                self.process_send_chat_message(ctx, client_id, message)
            }
            BughouseClientEvent::UpdateChalkDrawing { drawing } => {
                self.process_update_chalk_drawing(ctx, client_id, drawing)
            }
            BughouseClientEvent::RequestExport { format } => {
                self.process_request_export(ctx, client_id, format)
            }
            BughouseClientEvent::ReportPerformace(..) => {
                unreachable!("Match-independent event must be processed separately");
            }
            BughouseClientEvent::ReportError(..) => {
                unreachable!("Match-independent event must be processed separately");
            }
            BughouseClientEvent::Ping => {
                unreachable!("Match-independent event must be processed separately");
            }
        };
        if let Err(err) = result {
            ctx.clients[client_id].send_rejection(err);
        }
    }

    fn join_participant(
        &mut self, ctx: &mut Context, client_id: ClientId, execution: Execution,
        player_name: String, hot_reconnect: bool,
    ) -> EventResult {
        assert!(ctx.clients[client_id].match_id.is_none());
        assert!(ctx.clients[client_id].participant_id.is_none());

        let registered_user_name = ctx.clients[client_id]
            .session_id
            .as_ref()
            .and_then(|id| ctx.session_store.get(id))
            .and_then(Session::user_info)
            .map(|u| &u.user_name);
        let is_registered_user = registered_user_name.is_some();
        if let Some(registered_user_name) = registered_user_name {
            if player_name != *registered_user_name {
                return Err(unknown_error!(
                    "Name mismatch: player name = {player_name}, user name = {registered_user_name}."
                ));
            }
        }
        // Improvement potential: Reject earlier if a guest is trying to create a rated match.
        if self.rules.match_rules.rated && !is_registered_user {
            return Err(BughouseServerRejection::GuestInRatedMatch);
        }

        // On kicking existing user vs rejecting the new one:
        //   - If both accounts are registered, we can be certain this is the same user. Thus we are
        //     free to remove old connections.
        //   - If one account is registered and another one is a guest, then these are definitely
        //     different users.
        //   - If both are guests, we can only guess. On the one hand, we would like to allow
        //     reconnecting as soon as possible after a network failure. On the other hand, always
        //     preferring the new connection would allow trolls to kick out other players. Decision:
        //     we allow to kick another guest account if their connection is unhealthy (which means
        //     this is likely the same user trying to reconnect) or in case of a hot reconnect.
        if let Some(ref game_state) = self.game_state {
            let existing_participant_id = self.participants.find_by_name(&player_name);
            if let Some(existing_participant_id) = existing_participant_id {
                if let Some(existing_client_id) =
                    ctx.clients.find_participant(&self.match_id, existing_participant_id)
                {
                    assert_ne!(existing_client_id, client_id);
                    let is_existing_user_registered =
                        self.participants[existing_participant_id].is_registered_user;
                    let is_existing_user_connection_healthy = ctx.clients[existing_client_id]
                        .connection_monitor
                        .status(ctx.now)
                        .is_healthy();
                    let both_registered = is_registered_user && is_existing_user_registered;
                    let both_guests = !is_registered_user && !is_existing_user_registered;
                    if both_registered
                        || (both_guests && (hot_reconnect || !is_existing_user_connection_healthy))
                    {
                        ctx.clients[existing_client_id]
                            .send_rejection(BughouseServerRejection::JoinedInAnotherClient);
                        ctx.clients.remove_client(existing_client_id);
                    } else {
                        return Err(BughouseServerRejection::PlayerAlreadyExists { player_name });
                    }
                }
            }
            let participant_id = if let Some(id) = existing_participant_id {
                id
            } else {
                if let Err(reason) = ctx.helpers.validate_player_name(&player_name) {
                    return Err(BughouseServerRejection::InvalidPlayerName { player_name, reason });
                }
                // Improvement potential. Allow joining mid-game in individual mode.
                //   Q. How to balance score in this case?
                self.participants.add_participant(Participant {
                    name: player_name,
                    is_registered_user,
                    faction: Faction::Observer,
                    games_played: 0,
                    double_games_played: 0,
                    is_online: true,
                    is_ready: false,
                })
            };
            ctx.clients[client_id].match_id = Some(self.match_id.clone());
            ctx.clients[client_id].participant_id = Some(participant_id);
            ctx.clients[client_id].send(self.make_match_welcome_event());
            // LobbyUpdated should precede GameStarted, because this is how the client gets their
            // team in FixedTeam mode.
            self.send_lobby_updated(ctx);
            ctx.clients[client_id].send(self.make_game_start_event(ctx.now, Some(participant_id)));
            self.send_messages(ctx, Some(client_id), self.chat.all_messages());
            let chalkboard = game_state.chalkboard.clone();
            ctx.clients[client_id].send(BughouseServerEvent::ChalkboardUpdated { chalkboard });
            Ok(())
        } else {
            let existing_participant_id = self.participants.find_by_name(&player_name);
            if let Some(existing_participant_id) = existing_participant_id {
                if let Some(existing_client_id) =
                    ctx.clients.find_participant(&self.match_id, existing_participant_id)
                {
                    assert_ne!(existing_client_id, client_id);
                    let is_existing_user_registered =
                        self.participants[existing_participant_id].is_registered_user;
                    let is_existing_user_connection_healthy = ctx.clients[existing_client_id]
                        .connection_monitor
                        .status(ctx.now)
                        .is_healthy();
                    if is_registered_user {
                        let rejection = if is_existing_user_registered {
                            BughouseServerRejection::JoinedInAnotherClient // this is us
                        } else {
                            BughouseServerRejection::NameClashWithRegisteredUser
                        };
                        ctx.clients[existing_client_id].send_rejection(rejection);
                        ctx.clients.remove_client(existing_client_id);
                    } else if !is_existing_user_registered
                        && (hot_reconnect || !is_existing_user_connection_healthy)
                    {
                        ctx.clients[existing_client_id]
                            .send_rejection(BughouseServerRejection::JoinedInAnotherClient);
                        ctx.clients.remove_client(existing_client_id);
                    } else {
                        return Err(BughouseServerRejection::PlayerAlreadyExists { player_name });
                    }
                }
            }
            if let Err(reason) = ctx.helpers.validate_player_name(&player_name) {
                return Err(BughouseServerRejection::InvalidPlayerName { player_name, reason });
            }
            if !matches!(execution, Execution::Running) {
                return Err(BughouseServerRejection::ShuttingDown);
            }
            info!(
                "Client {} join match {} as {}",
                ctx.clients[client_id].logging_id, self.match_id.0, player_name
            );
            ctx.clients[client_id].match_id = Some(self.match_id.clone());
            let participant_id = self.participants.add_participant(Participant {
                name: player_name,
                is_registered_user,
                faction: Faction::Random,
                games_played: 0,
                double_games_played: 0,
                is_online: true,
                is_ready: false,
            });
            ctx.clients[client_id].participant_id = Some(participant_id);
            ctx.clients[client_id].send(self.make_match_welcome_event());
            self.send_lobby_updated(ctx);
            Ok(())
        }
    }

    fn process_set_faction(
        &mut self, ctx: &mut Context, client_id: ClientId, faction: Faction,
    ) -> EventResult {
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        if self.game_state.is_some() {
            return Err(unknown_error!());
        }
        self.participants[participant_id].faction = faction;
        self.send_lobby_updated(ctx);
        Ok(())
    }

    fn process_set_turns(
        &mut self, ctx: &mut Context, client_id: ClientId, turns: Vec<(BughouseBoard, TurnInput)>,
    ) -> EventResult {
        let Some(GameState { ref game, ref mut turn_requests, .. }) = self.game_state else {
            return Err(unknown_error!());
        };
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        let player_bughouse_id = game
            .find_player(&self.participants[participant_id].name)
            .ok_or_else(|| unknown_error!())?;
        turn_requests.retain(|r| !player_bughouse_id.plays_for(r.envoy));
        for (board_idx, turn_input) in turns {
            self.process_make_turn(ctx, client_id, board_idx, turn_input)?;
        }
        Ok(())
    }

    fn process_make_turn(
        &mut self, ctx: &mut Context, client_id: ClientId, board_idx: BughouseBoard,
        turn_input: TurnInput,
    ) -> EventResult {
        let Some(GameState {
            game_index,
            ref mut game_start,
            ref mut game_start_offset_time,
            ref mut game_end,
            ref mut game,
            ref mut turn_requests,
            ..
        }) = self.game_state
        else {
            return Err(unknown_error!());
        };
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        let player_bughouse_id = game
            .find_player(&self.participants[participant_id].name)
            .ok_or_else(|| unknown_error!())?;
        let envoy = player_bughouse_id.envoy_for(board_idx).ok_or_else(|| unknown_error!())?;

        if turn_requests.iter().filter(|r| r.envoy == envoy).count()
            > self.rules.chess_rules.max_preturns_per_board()
        {
            return Err(unknown_error!("Premove limit reached"));
        }
        let request = TurnRequest { envoy, turn_input };
        turn_requests.push(request);

        let mut turns = vec![];
        let mut game_over_update = None;
        // Note. Turn resolution is currently O(N^2) where N is the number of turns in the queue,
        // but this is fine because in practice N is very low.
        while let Some(turn_event) = resolve_one_turn(ctx.now, *game_start, game, turn_requests) {
            turns.push(turn_event);
            if game_start.is_none() {
                *game_start = Some(ctx.now);
                *game_start_offset_time = Some(time::OffsetDateTime::now_utc());
            }
            if !game.is_active() {
                let game_now = GameInstant::from_now_game_active(game_start.unwrap(), ctx.now);
                game_over_update = Some(update_on_game_over(
                    ctx,
                    self.teaming.unwrap(),
                    game_index,
                    game,
                    turn_requests,
                    &mut self.participants,
                    self.scores.as_mut().unwrap(),
                    &mut self.next_board_assignment,
                    &mut self.chat,
                    game_now,
                    *game_start_offset_time,
                    game_end,
                ));
                break;
            }
        }
        if !turns.is_empty() {
            let mut updates = turns
                .into_iter()
                .map(|turn_record| GameUpdate::TurnMade { turn_record })
                .collect_vec();
            if let Some(game_over_update) = game_over_update {
                updates.push(game_over_update);
            }
            self.add_game_updates(ctx, updates);
        }
        Ok(())
    }

    fn process_cancel_preturn(
        &mut self, ctx: &mut Context, client_id: ClientId, board_idx: BughouseBoard,
    ) -> EventResult {
        let Some(GameState { ref game, ref mut turn_requests, .. }) = self.game_state else {
            return Err(unknown_error!());
        };
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        let player_bughouse_id = game
            .find_player(&self.participants[participant_id].name)
            .ok_or_else(|| unknown_error!())?;
        let envoy = player_bughouse_id.envoy_for(board_idx).ok_or_else(|| unknown_error!())?;
        for (idx, r) in turn_requests.iter().enumerate().rev() {
            if r.envoy == envoy {
                turn_requests.remove(idx);
                break;
            }
        }
        Ok(())
    }

    fn process_resign(&mut self, ctx: &mut Context, client_id: ClientId) -> EventResult {
        let Some(GameState {
            game_index,
            ref mut game,
            ref mut turn_requests,
            game_start,
            game_start_offset_time,
            ref mut game_end,
            ..
        }) = self.game_state
        else {
            return Err(unknown_error!());
        };
        if !game.is_active() {
            return Err(unknown_error!());
        }
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        let player_bughouse_id = game
            .find_player(&self.participants[participant_id].name)
            .ok_or_else(|| unknown_error!())?;
        let status = BughouseGameStatus::Victory(
            player_bughouse_id.team().opponent(),
            VictoryReason::Resignation,
        );
        let game_now = GameInstant::from_now_game_maybe_active(game_start, ctx.now);
        game.set_status(status, game_now);
        let update = update_on_game_over(
            ctx,
            self.teaming.unwrap(),
            game_index,
            game,
            turn_requests,
            &mut self.participants,
            self.scores.as_mut().unwrap(),
            &mut self.next_board_assignment,
            &mut self.chat,
            game_now,
            game_start_offset_time,
            game_end,
        );
        self.add_game_updates(ctx, vec![update]);
        Ok(())
    }

    fn process_set_ready(
        &mut self, ctx: &mut Context, client_id: ClientId, is_ready: bool,
    ) -> EventResult {
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        if let Some(GameState { ref game, .. }) = self.game_state {
            if game.is_active() {
                // No error: somebody could've tried to unset ready flag while the game has started.
                return Ok(());
            }
        }
        self.participants[participant_id].is_ready = is_ready;
        self.send_lobby_updated(ctx);
        Ok(())
    }

    fn process_leave_match(&mut self, ctx: &mut Context, client_id: ClientId) -> EventResult {
        ctx.clients[client_id].match_id = None;
        ctx.clients[client_id].participant_id = None;
        // Participant will be removed automatically if required.
        Ok(())
    }

    fn process_leave_server(&mut self, ctx: &mut Context, client_id: ClientId) -> EventResult {
        if let Some(logging_id) = ctx.clients.remove_client(client_id) {
            info!("Client {} left", logging_id);
        }
        // Note. Player will be removed automatically. This has to be the case, otherwise
        // clients disconnected due to a network error would've left abandoned players.
        // Improvement potential. Do we really need this event? Clients are removed when the
        // network channel is closed anyway.
        Ok(())
    }

    fn process_send_chat_message(
        &mut self, ctx: &mut Context, client_id: ClientId, message: OutgoingChatMessage,
    ) -> EventResult {
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        let sender = &self.participants[participant_id];
        let recipient_expanded = match &message.recipient {
            ChatRecipient::All => ChatRecipientExpanded::All,
            ChatRecipient::Team => {
                let teaming = self.teaming.ok_or_else(|| unknown_error!())?;
                match teaming {
                    Teaming::DynamicTeams => {
                        let Some(GameState { ref game, .. }) = self.game_state else {
                            return Err(unknown_error!());
                        };
                        let Some(sender_team) = game.find_player(&sender.name).map(|p| p.team())
                        else {
                            return Err(unknown_error!());
                        };
                        ChatRecipientExpanded::Participants(
                            self.participants
                                .iter()
                                .map(|p| p.name.clone())
                                .filter(|name| {
                                    game.find_player(name).is_some_and(|p| p.team() == sender_team)
                                })
                                .collect(),
                        )
                    }
                    Teaming::FixedTeams => match sender.faction {
                        Faction::Fixed(team) => ChatRecipientExpanded::FixedTeam(team),
                        Faction::Random => unreachable!(),
                        Faction::Observer => return Err(unknown_error!()),
                    },
                }
            }
            ChatRecipient::Participant(name) => {
                ChatRecipientExpanded::Participants(iter::once(name.clone()).collect())
            }
        };
        let game_index = self.game_state.as_ref().map(|s| s.game_index);
        self.chat
            .add(game_index, ctx.utc_now, recipient_expanded, ChatMessageBody::Regular {
                sender: sender.name.clone(),
                recipient: message.recipient,
                text: message.text,
            });
        self.participants
            .extra_mut(participant_id)
            .confirmed_local_message_id
            .relax_max(message.local_message_id);
        Ok(())
    }

    fn process_update_chalk_drawing(
        &mut self, ctx: &mut Context, client_id: ClientId, drawing: ChalkDrawing,
    ) -> EventResult {
        let Some(GameState { ref mut chalkboard, ref game, .. }) = self.game_state else {
            return Err(unknown_error!());
        };
        let participant_id =
            ctx.clients[client_id].participant_id.ok_or_else(|| unknown_error!())?;
        if game.is_active() {
            return Err(unknown_error!());
        }
        chalkboard.set_drawing(self.participants[participant_id].name.clone(), drawing);
        let chalkboard = chalkboard.clone();
        self.broadcast(ctx, &BughouseServerEvent::ChalkboardUpdated { chalkboard });
        Ok(())
    }

    fn process_request_export(
        &self, ctx: &mut Context, client_id: ClientId, format: BughouseExportFormat,
    ) -> EventResult {
        let Some(GameState { ref game, .. }) = self.game_state else {
            return Err(unknown_error!());
        };
        let all_games = self.match_history.iter().chain(iter::once(game));
        let content = all_games
            .enumerate()
            .map(|(round, game)| pgn::export_to_bpgn(format, game, round + 1))
            .join("\n");
        ctx.clients[client_id].send(BughouseServerEvent::GameExportReady { content });
        Ok(())
    }

    fn post_process(&mut self, ctx: &mut Context, execution: Execution) {
        let new_chat_messages = fetch_new_chat_messages!(self.chat);
        self.send_messages(ctx, None, new_chat_messages);

        // Improvement potential: Collapse `send_lobby_updated` events generated during one event
        //   processing cycle. Right now there could up to three: one from the event (SetTeam/SetReady),
        //   one from here and one from `self.start_game`.
        //   Idea: Add `ctx.should_update_lobby` bit and check it in the end.
        // TODO: Show lobby participants as offline when `!c.heart.is_online()`.
        let active_participant_ids: HashSet<_> =
            ctx.clients.map.values().filter_map(|c| c.participant_id).collect();
        let mut lobby_updated = false;
        let mut chalkboard_updated = false;
        self.participants.map.retain(|id, (p, _)| {
            let is_online = active_participant_ids.contains(id);
            if !is_online {
                if self.game_state.is_none() {
                    lobby_updated = true;
                    return false;
                }
                if !p.faction.is_player() {
                    if let Some(ref mut game_state) = self.game_state {
                        chalkboard_updated |=
                            game_state.chalkboard.clear_drawings_by_player(p.name.clone());
                    }
                    lobby_updated = true;
                    return false;
                }
            }
            if p.is_online != is_online {
                p.is_online = is_online;
                p.is_ready &= is_online;
                lobby_updated = true;
            }
            true
        });
        if lobby_updated {
            self.send_lobby_updated(ctx);
        }
        if chalkboard_updated {
            let chalkboard = self.game_state.as_ref().unwrap().chalkboard.clone();
            self.broadcast(ctx, &BughouseServerEvent::ChalkboardUpdated { chalkboard });
        }

        let can_start_game =
            verify_participants(&self.rules, self.participants.iter()).error.is_none();
        if let Some(first_game_countdown_start) = self.first_game_countdown_since {
            if !can_start_game {
                self.first_game_countdown_since = None;
                self.send_lobby_updated(ctx);
            } else if ctx.now.duration_since(first_game_countdown_start)
                >= FIRST_GAME_COUNTDOWN_DURATION
            {
                self.start_game(ctx);
            }
        } else if can_start_game {
            if !matches!(execution, Execution::Running) {
                self.broadcast(
                    ctx,
                    &BughouseServerEvent::Rejection(BughouseServerRejection::ShuttingDown),
                );
                self.reset_readiness();
                return;
            }
            if let Some(GameState { ref game, .. }) = self.game_state {
                assert!(
                    !game.is_active(),
                    "Players must not be allowed to set is_ready flag while the game is active"
                );
                self.match_history.push(game.clone());
                self.start_game(ctx);
            } else if self.first_game_countdown_since.is_none() {
                // Show final teams when countdown begins.
                fix_teams_if_needed(&mut self.participants);
                self.send_lobby_updated(ctx);

                if ctx.disable_countdown {
                    self.start_game(ctx);
                } else {
                    // TODO: Add some way of blocking last-moment faction changes, e.g.:
                    //   - Forbid all changes other than resetting readiness;
                    //   - Allow changes, but restart the count-down;
                    //   - Blizzard-style: allow changes during the first half of the count-down.
                    self.first_game_countdown_since = Some(ctx.now);
                    self.send_lobby_updated(ctx);
                }
            }
        }
    }

    fn start_game(&mut self, ctx: &mut Context) {
        self.reset_readiness();

        // Non-trivial only in the beginning of a match, when the first game starts. Moreover, we've
        // already called `fix_teams_if_needed` when the countdown began, but calling it again to be
        // sure.
        if self.teaming.is_none() {
            let teaming = fix_teams_if_needed(&mut self.participants);
            self.teaming = Some(teaming);
            self.init_scores(teaming);
        }

        let players = self
            .next_board_assignment
            .take()
            .unwrap_or_else(|| assign_boards(self.participants.iter(), &mut rand::thread_rng()));
        let game = BughouseGame::new(self.rules.clone(), Role::ServerOrStandalone, &players);
        let game_index = self.match_history.len() as u64;
        self.game_state = Some(GameState {
            game_index,
            game,
            game_creation: ctx.now,
            game_start: None,
            game_start_offset_time: None,
            game_end: None,
            updates: Vec::new(),
            turn_requests: Vec::new(),
            chalkboard: Chalkboard::new(),
        });
        self.broadcast(ctx, &self.make_game_start_event(ctx.now, None));
        self.send_lobby_updated(ctx); // update readiness flags
    }

    fn init_scores(&mut self, teaming: Teaming) {
        if self.scores.is_some() {
            return;
        }
        match teaming {
            Teaming::FixedTeams => {
                let scores = enum_map! { _ => HalfU32::ZERO };
                self.scores = Some(Scores::PerTeam(scores));
            }
            Teaming::DynamicTeams => {
                let mut scores = HashMap::new();
                for p in self.participants.iter() {
                    scores.insert(p.name.clone(), HalfU32::ZERO);
                }
                self.scores = Some(Scores::PerPlayer(scores));
            }
        }
    }

    fn make_match_welcome_event(&self) -> BughouseServerEvent {
        BughouseServerEvent::MatchWelcome {
            match_id: self.match_id.0.clone(),
            rules: self.rules.clone(),
        }
    }

    // Creates a game start/reconnect event. `participant_id` is needed only if reconnecting.
    fn make_game_start_event(
        &self, now: Instant, participant_id: Option<ParticipantId>,
    ) -> BughouseServerEvent {
        let Some(game_state) = &self.game_state else {
            panic!("Expected MatchState::Game");
        };
        let player_bughouse_id =
            participant_id.and_then(|id| game_state.game.find_player(&self.participants[id].name));
        let preturns = if let Some(player_bughouse_id) = player_bughouse_id {
            player_turn_requests(&game_state.turn_requests, player_bughouse_id)
        } else {
            vec![]
        };
        BughouseServerEvent::GameStarted {
            game_index: game_state.game_index,
            starting_position: game_state.game.starting_position().clone(),
            players: game_state.game.players(),
            time: current_game_time(game_state, now),
            updates: game_state.updates.clone(),
            preturns,
            scores: self.scores.clone().unwrap(),
        }
    }

    fn send_lobby_updated(&self, ctx: &mut Context) {
        let participants = self.participants.iter().cloned().collect();
        let countdown_elapsed = self.first_game_countdown_since.map(|t| ctx.now.duration_since(t));
        self.broadcast(ctx, &BughouseServerEvent::LobbyUpdated { participants, countdown_elapsed });
    }

    fn reset_readiness(&mut self) { self.participants.iter_mut().for_each(|p| p.is_ready = false); }

    // Improvement potential. Instead of traversing all messages for each client we could keep a map
    // from client_id to (messages, message_confirmations) and update it as we traverse the messages
    // in one go. In practice it doesn't matter much, since we either have one message (in a regular
    // chat workflow) or one client (in case of a reconnect).
    fn send_messages(
        &self, ctx: &mut Context, limit_to_client_id: Option<ClientId>,
        messages_to_send: impl IntoIterator<Item = &(ChatRecipientExpanded, ChatMessage)>,
    ) {
        let messages_to_send = messages_to_send.into_iter().collect_vec();
        let send_to_client = |client: &mut Client| {
            if let Some(participant_id) = client.participant_id {
                let p = &self.participants[participant_id];
                let p_extra = &self.participants.extra(participant_id);
                let mut messages = vec![];
                for (recipient_expanded, m) in messages_to_send.iter() {
                    let is_sender = match &m.body {
                        ChatMessageBody::Regular { sender, .. } => &p.name == sender,
                        ChatMessageBody::GameOver { .. } => false,
                        ChatMessageBody::NextGamePlayers { .. } => false,
                    };
                    let is_recipient = match recipient_expanded {
                        ChatRecipientExpanded::All => true,
                        ChatRecipientExpanded::FixedTeam(team) => {
                            matches!(p.faction, Faction::Fixed(t) if t == *team)
                        }
                        ChatRecipientExpanded::Participants(names) => names.contains(&p.name),
                    };
                    if is_sender || is_recipient {
                        messages.push(m.clone());
                    }
                }
                // Checking whether `confirmed_local_message_id` advanced seperately is superfluous:
                // we are sending all messages back to the client, so there will always be new
                // messages whenever `confirmed_local_message_id` advances.
                if !messages.is_empty() {
                    client.send(BughouseServerEvent::ChatMessages {
                        messages,
                        confirmed_local_message_id: p_extra.confirmed_local_message_id,
                    });
                }
            }
        };

        if let Some(limit_to_client_id) = limit_to_client_id {
            let client = &mut ctx.clients[limit_to_client_id];
            send_to_client(client);
        } else {
            for client in ctx.clients.map.values_mut() {
                if client.match_id.as_ref() == Some(&self.match_id) {
                    send_to_client(client);
                }
            }
        }
    }
}

fn current_game_time(game_state: &GameState, now: Instant) -> Option<GameInstant> {
    if !game_state.game.started() {
        None
    } else if game_state.game.is_active() {
        Some(GameInstant::from_now_game_maybe_active(game_state.game_start, now))
    } else {
        let elapsed_since_start = BughouseBoard::iter()
            .map(|board_idx| game_state.game.board(board_idx).clock().total_time_elapsed())
            .all_equal_value()
            .unwrap();
        Some(GameInstant::from_duration(elapsed_since_start))
    }
}

fn resolve_one_turn(
    now: Instant, game_start: Option<Instant>, game: &mut BughouseGame,
    turn_requests: &mut Vec<TurnRequest>,
) -> Option<TurnRecord> {
    let game_now = GameInstant::from_now_game_maybe_active(game_start, now);
    let mut iter = mem::take(turn_requests).into_iter();
    while let Some(r) = iter.next() {
        match game.turn_mode_for_envoy(r.envoy) {
            Ok(TurnMode::Normal) => {
                if game
                    .try_turn_by_envoy(r.envoy, &r.turn_input, TurnMode::Normal, game_now)
                    .is_ok()
                {
                    // Discard this turn, but keep the rest.
                    turn_requests.extend(iter);
                    return Some(game.last_turn_record().unwrap().trim_for_sending());
                } else {
                    // Discard. Ignore error: Preturns fail all the time and even a valid in-order
                    // turn can fail, e.g. if the game ended on the board board in the meantime, or
                    // the piece was stolen, or you got a new king in Koedem.
                }
            }
            Ok(TurnMode::Preturn) => {
                // Keep the turn for now.
                turn_requests.push(r);
            }
            Err(_) => {
                // Discard. Ignore error (see above).
            }
        }
    }
    None
}

// Improvement potential. Find a way to make this a member function instead of passing so many
// arguments separately.
fn update_on_game_over(
    ctx: &mut Context, teaming: Teaming, game_index: u64, game: &BughouseGame,
    turn_requests: &mut Vec<TurnRequest>, participants: &mut Participants, scores: &mut Scores,
    next_board_assignment: &mut Option<Vec<PlayerInGame>>, chat: &mut ServerChat,
    game_now: GameInstant, game_start_offset_time: Option<time::OffsetDateTime>,
    game_end: &mut Option<Instant>,
) -> GameUpdate {
    assert!(game_end.is_none());
    *game_end = Some(ctx.now);
    turn_requests.clear();
    let team_scores = match game.status() {
        BughouseGameStatus::Active => {
            panic!("It just so happens that the game here is only mostly over")
        }
        BughouseGameStatus::Victory(team, _) => {
            let mut s = enum_map! { _ => HalfU32::ZERO };
            s[team] = HalfU32::whole(1);
            s
        }
        BughouseGameStatus::Draw(_) => enum_map! { _ => HalfU32::HALF },
    };
    match scores {
        Scores::PerTeam(ref mut score_map) => {
            for (team, score) in team_scores {
                score_map[team] += score;
            }
        }
        Scores::PerPlayer(ref mut score_map) => {
            for p in game.players() {
                *score_map.entry(p.name.clone()).or_insert(HalfU32::ZERO) +=
                    team_scores[p.id.team()];
            }
        }
    }
    let mut num_players_per_team = enum_map! { _ => 0 };
    for p in game.players() {
        num_players_per_team[p.id.team()] += 1;
    }
    for p in game.players() {
        if let Some(id) = participants.find_by_name(&p.name) {
            participants[id].games_played += 1;
            if num_players_per_team[p.id.team()] == 1 {
                participants[id].double_games_played += 1;
            }
        }
    }
    let round = game_index as usize + 1;
    ctx.hooks.on_game_over(game, game_start_offset_time, round);
    let next_players = assign_boards(participants.iter(), &mut rand::thread_rng());
    *next_board_assignment = Some(next_players.clone());

    chat.add(
        Some(game_index),
        ctx.utc_now,
        ChatRecipientExpanded::All,
        ChatMessageBody::GameOver { outcome: game.outcome() },
    );
    let total_players = participants.iter().filter(|p| p.faction.is_player()).count();
    // Improvement potential. Remove "Next up" message when playing one vs one.
    if next_players.len() < total_players || teaming == Teaming::DynamicTeams {
        chat.add(
            Some(game_index),
            ctx.utc_now,
            ChatRecipientExpanded::All,
            ChatMessageBody::NextGamePlayers { players: next_players },
        );
    }

    GameUpdate::GameOver {
        time: game_now,
        game_status: game.status(),
        scores: scores.clone(),
    }
}

fn player_turn_requests(
    turn_requests: &[TurnRequest], player: BughousePlayer,
) -> Vec<(BughouseBoard, TurnInput)> {
    turn_requests
        .iter()
        .filter(|r| player.plays_for(r.envoy))
        .map(|r| (r.envoy.board_idx, r.turn_input.clone()))
        .collect()
}

fn process_report_error(ctx: &Context, client_id: ClientId, report: &BughouseClientErrorReport) {
    // TODO: Save errors to DB.
    let logging_id = &ctx.clients[client_id].logging_id;
    match report {
        BughouseClientErrorReport::RustPanic { panic_info, backtrace } => {
            warn!("Client {logging_id} panicked:\n{panic_info}\nBacktrace: {backtrace}");
        }
        BughouseClientErrorReport::RustError { message } => {
            warn!("Client {logging_id} experienced Rust error:\n{message}");
        }
        BughouseClientErrorReport::UnknownError { message } => {
            warn!("Client {logging_id} experienced unknown error:\n{message}");
        }
    }
}

fn process_ping(ctx: &mut Context, client_id: ClientId) {
    ctx.clients[client_id].send(BughouseServerEvent::Pong);
}

fn event_name(event: &IncomingEvent) -> &'static str {
    match &event {
        IncomingEvent::Network(_, event) => match event {
            BughouseClientEvent::NewMatch { .. } => "Client_NewMatch",
            BughouseClientEvent::Join { .. } => "Client_Join",
            BughouseClientEvent::HotReconnect { .. } => "Client_HotReconnect",
            BughouseClientEvent::SetFaction { .. } => "Client_SetFaction",
            BughouseClientEvent::SetTurns { .. } => "Client_SetTurns",
            BughouseClientEvent::MakeTurn { .. } => "Client_MakeTurn",
            BughouseClientEvent::CancelPreturn { .. } => "Client_CancelPreturn",
            BughouseClientEvent::Resign => "Client_Resign",
            BughouseClientEvent::SetReady { .. } => "Client_SetReady",
            BughouseClientEvent::LeaveMatch => "Client_LeaveMatch",
            BughouseClientEvent::LeaveServer => "Client_LeaveServer",
            BughouseClientEvent::SendChatMessage { .. } => "Client_SendChatMessage",
            BughouseClientEvent::UpdateChalkDrawing { .. } => "Client_UpdateChalkDrawing",
            BughouseClientEvent::RequestExport { .. } => "Client_RequestExport",
            BughouseClientEvent::ReportPerformace(_) => "Client_ReportPerformace",
            BughouseClientEvent::ReportError(_) => "Client_ReportError",
            BughouseClientEvent::Ping => "Client_Ping",
        },
        IncomingEvent::Tick => "Tick",
        IncomingEvent::Terminate => "Terminate",
    }
}

fn shutdown() {
    // Note. It may seem like a good idea to terminate the process "properly": join threads, call
    // destructors, etc. But I think it's actually not. By aborting the process during the normal
    // shutdown procedure we make sure that this path is tested and thus abnormal shutdown (panic
    // or VM failure) does not lose data.
    std::process::exit(0);
}
