// Improvement potential. Replace `game.find_player(&self.players[player_id].name)`
//   with a direct mapping (player_id -> player_bughouse_id).

use std::collections::{HashSet, HashMap, hash_map};
use std::iter;
use std::ops;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Duration;

use enum_map::enum_map;
use instant::Instant;
use itertools::Itertools;
use log::{info, warn};
use rand::{Rng, seq::SliceRandom};
use strum::IntoEnumIterator;

use crate::board::{TurnMode, TurnError, TurnInput, VictoryReason};
use crate::chalk::{ChalkDrawing, Chalkboard};
use crate::clock::GameInstant;
use crate::game::{TurnRecord, BughouseBoard, BughousePlayerId, BughouseGameStatus, BughouseGame};
use crate::heartbeat::{Heart, HeartbeatOutcome};
use crate::event::{BughouseServerEvent, BughouseClientEvent, BughouseClientErrorReport};
use crate::pgn::{self, BughouseExportFormat};
use crate::player::{Player, PlayerInGame, Team};
use crate::rules::{Teaming, ChessRules, BughouseRules};
use crate::scores::Scores;
use crate::server_hooks::{ServerHooks, NoopServerHooks};


const TOTAL_PLAYERS: usize = 4;
const TOTAL_PLAYERS_PER_TEAM: usize = 2;
const CONTEST_GC_INACTIVITY_THRESHOLD: Duration = Duration::from_secs(3600 * 24);

#[derive(Debug)]
pub enum IncomingEvent {
    Network(ClientId, BughouseClientEvent),
    Tick,
}

#[derive(Debug)]
pub struct GameState {
    game: BughouseGame,
    game_start: Option<Instant>,
    preturns: HashMap<BughousePlayerId, TurnInput>,
    chalkboard: Chalkboard,
    players_with_boards: Vec<(PlayerInGame, BughouseBoard)>, // TODO: Extract from `game`
}

impl GameState {
    pub fn game(&self) -> &BughouseGame { &self.game }
    pub fn players_with_boards(&self) -> &Vec<(PlayerInGame, BughouseBoard)> {
        &self.players_with_boards
    }
}


#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct ContestId(String);


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PlayerId(usize);

struct Players {
    map: HashMap<PlayerId, Player>,
    next_id: usize,
}

impl Players {
    fn new() -> Self { Self{ map: HashMap::new(), next_id: 1 } }
    fn len(&self) -> usize { self.map.len() }
    fn iter(&self) -> impl Iterator<Item = &Player> { self.map.values() }
    fn iter_mut(&mut self) -> impl Iterator<Item = &mut Player> { self.map.values_mut() }
    fn find_by_name(&self, name: &str) -> Option<PlayerId> {
        self.map.iter().find_map(|(id, p)| if p.name == name { Some(*id) } else { None })
    }
    fn add_player(&mut self, player: Player) -> PlayerId {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        assert!(self.map.insert(id, player).is_none());
        id
    }
}

impl ops::Index<PlayerId> for Players {
    type Output = Player;
    fn index(&self, id: PlayerId) -> &Self::Output { &self.map[&id] }
}
impl ops::IndexMut<PlayerId> for Players {
    fn index_mut(&mut self, id: PlayerId) -> &mut Self::Output { self.map.get_mut(&id).unwrap() }
}


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClientId(usize);

pub struct Client {
    events_tx: mpsc::Sender<BughouseServerEvent>,
    contest_id: Option<ContestId>,
    player_id: Option<PlayerId>,
    logging_id: String,
    heart: Heart,
}

impl Client {
    fn send(&mut self, event: BughouseServerEvent) {
        // Improvement potential: Propagate `now` from the caller.
        let now = Instant::now();
        self.events_tx.send(event).unwrap();
        self.heart.register_outgoing(now);
    }
    fn send_error(&mut self, message: String) {
        self.send(BughouseServerEvent::Error{ message });
    }
}

pub struct Clients {
    map: HashMap<ClientId, Client>,
    next_id: usize,
}

impl Clients {
    pub fn new() -> Self { Clients{ map: HashMap::new(), next_id: 1 } }

    pub fn add_client(&mut self, events_tx: mpsc::Sender<BughouseServerEvent>, logging_id: String)
        -> ClientId
    {
        let now = Instant::now();
        let client = Client {
            events_tx,
            contest_id: None,
            player_id: None,
            logging_id,
            heart: Heart::new(now),
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

    // Sends the event to each client who has joined the contest.
    //
    // Improvement potential. Do not iterate over all clients. Keep the list of clients
    // in each contest.
    fn broadcast(&mut self, contest_id: &ContestId, event: &BughouseServerEvent) {
        for client in self.map.values_mut() {
            if client.contest_id.as_ref() == Some(contest_id) {
                client.send(event.clone());
            }
        }
    }
}

impl ops::Index<ClientId> for Clients {
    type Output = Client;
    fn index(&self, id: ClientId) -> &Self::Output { &self.map[&id] }
}
impl ops::IndexMut<ClientId> for Clients {
    fn index_mut(&mut self, id: ClientId) -> &mut Self::Output { self.map.get_mut(&id).unwrap() }
}


struct Contest {
    contest_id: ContestId,
    chess_rules: ChessRules,
    bughouse_rules: BughouseRules,
    players: Players,
    scores: Scores,
    match_history: Vec<BughouseGame>,  // final game states
    game_state: Option<GameState>,  // active game or latest game
    last_activity: Instant,  // for GC
    board_assignment_override: Option<Vec<(String, BughousePlayerId)>>,  // for tests
}

struct Context<'a, 'b> {
    clients: &'b mut MutexGuard<'a, Clients>,
    hooks: &'a mut dyn ServerHooks,
}

struct CoreServerState {
    contests: HashMap<ContestId, Contest>,
}

type EventResult = Result<(), String>;

// Split state into two parts (core and context) in order to allow things like:
//   let mut clients = self.clients.lock().unwrap();
//   self.core.foo(&mut clients);
// whereas
//   self.foo(&mut clients);
// would make the compiler complain that `self` is borrowed twice.
pub struct ServerState {
    // Optimization potential: Lock-free map instead of Mutex<HashMap>.
    clients: Arc<Mutex<Clients>>,
    hooks: Box<dyn ServerHooks>,
    core: CoreServerState,
}

impl ServerState {
    pub fn new(
        clients: Arc<Mutex<Clients>>,
        hooks: Option<Box<dyn ServerHooks>>,
    ) -> Self {
        ServerState {
            clients,
            hooks: hooks.unwrap_or_else(|| Box::new(NoopServerHooks{})),
            core: CoreServerState::new(),
        }
    }

    pub fn apply_event(&mut self, event: IncomingEvent) {
        // Lock clients for the entire duration of the function. This means simpler and
        // more predictable event processing, e.g. it gives a guarantee that all broadcasts
        // from a single `apply_event` reach the same set of clients.
        //
        // Improvement potential. Rethink this approach. With multiple parallel contests this
        // global mutex may become a bottleneck.
        let mut clients = self.clients.lock().unwrap();

        let mut ctx = Context {
            clients: &mut clients,
            hooks: self.hooks.as_mut()
        };

        self.core.apply_event(&mut ctx, event);
    }

    #[allow(non_snake_case)]
    pub fn TEST_override_board_assignment(
        &mut self, contest_id: String, assignment: Vec<(String, BughousePlayerId)>
    ) {
        let contest_id = ContestId(contest_id);
        assert_eq!(assignment.len(), TOTAL_PLAYERS);
        self.core.contests.get_mut(&contest_id).unwrap().board_assignment_override = Some(assignment);
    }
}

impl CoreServerState {
    fn new() -> Self {
        CoreServerState{ contests: HashMap::new() }
    }

    fn make_contest(
        &mut self, now: Instant, chess_rules: ChessRules, bughouse_rules: BughouseRules
    ) -> ContestId {
        // Exclude confusing characters:
        //   - 'O' and '0' (easy to confuse);
        //   - 'I' (looks like '1'; keep '1' because confusion in the other direction seems less likely).
        const ALPHABET: [char; 33] = [
            '1', '2', '3', '4', '5', '6', '7', '8', '9',
            'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L', 'M',
            'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
        ];
        const MIN_ID_LEN: usize = 4;
        const MAX_ATTEMPTS_PER_LEN: usize = 100;
        let mut rng = rand::thread_rng();
        let mut id_len = MIN_ID_LEN;
        let mut id = ContestId(String::new());
        let mut attempts_at_this_len = 0;
        while id.0.is_empty() || self.contests.contains_key(&id) {
            id = ContestId((&mut rng)
                .sample_iter(rand::distributions::Uniform::from(0..ALPHABET.len()))
                .map(|idx| ALPHABET[idx])
                .take(id_len)
                .collect()
            );
            attempts_at_this_len += 1;
            if attempts_at_this_len > MAX_ATTEMPTS_PER_LEN {
                id_len += 1;
                attempts_at_this_len = 0;
            }
        }
        let contest = Contest {
            contest_id: id.clone(),
            chess_rules,
            bughouse_rules,
            players: Players::new(),
            scores: Scores::new(),
            match_history: Vec::new(),
            game_state: None,
            last_activity: now,
            board_assignment_override: None,
        };
        assert!(self.contests.insert(id.clone(), contest).is_none());
        id
    }

    fn apply_event(&mut self, ctx: &mut Context, event: IncomingEvent) {
        // Use the same timestamp for the entire event processing. Other code reachable
        // from this function should not call `Instant::now()`. Doing so may cause a race
        // condition: e.g. if we check the flag, see that it's ok and then continue to
        // write down a turn which, by that time, becomes illegal because player's time
        // is over.
        let now = Instant::now();

        match event {
            IncomingEvent::Network(client_id, event) => self.on_client_event(ctx, client_id, now, event),
            IncomingEvent::Tick => self.on_tick(ctx, now),
        }
    }

    fn on_client_event(
        &mut self, ctx: &mut Context, client_id: ClientId, now: Instant, event: BughouseClientEvent
    ) {
        ctx.hooks.on_client_event(&event);

        if !ctx.clients.map.contains_key(&client_id) {
            // TODO: Should there be an exception for `BughouseClientEvent::ReportError`?
            // TODO: Improve logging. Consider:
            //   - include logging_id inside client_id, or
            //   - keep disconnected clients in the map for some time.
            warn!("Got an event from disconnected client:\n{event:?}");
            return;
        }

        ctx.clients[client_id].heart.register_incoming(now);

        // First, process events that don't require a contest.
        match &event {
            BughouseClientEvent::ReportPerformace(..) => {
                // Only used by server hooks.
                return;
            },
            BughouseClientEvent::ReportError(report) => {
                process_report_error(ctx, client_id, report);
                return;
            },
            BughouseClientEvent::Heartbeat => {
                // This event is needed only for `heart.register_incoming` above.
                return;
            },
            _ => {},
        };

        let contest_id = match &event {
            BughouseClientEvent::NewContest{ chess_rules, bughouse_rules, .. } => {
                ctx.clients[client_id].contest_id = None;
                ctx.clients[client_id].player_id = None;
                let contest_id = self.make_contest(now, chess_rules.clone(), bughouse_rules.clone());
                info!("Contest {} created by client {}", contest_id.0, ctx.clients[client_id].logging_id);
                Some(contest_id)
            },
            BughouseClientEvent::Join{ contest_id, .. } => {
                // Improvement potential: Log cases when a client reconnects to their current
                //   contest. This likely indicates a client error.
                ctx.clients[client_id].contest_id = None;
                ctx.clients[client_id].player_id = None;
                Some(ContestId(contest_id.clone()))
            },
            _ => ctx.clients[client_id].contest_id.clone(),
        };

        let Some(contest_id) = contest_id else {
            // We've already processed all events that do not depend on a contest.
            ctx.clients[client_id].send_error("Cannot process event: no contest in progress".to_owned());
            return;
        };

        let Some(contest) = self.contests.get_mut(&contest_id) else {
            // The only way to have a contest_id with no contest is when a client is trying
            // to join with a bad contest_id. In other cases we are getting contest_id from
            // trusted internal sources, so the contest must exist as well.
            assert!(matches!(event, BughouseClientEvent::Join{ .. }));
            ctx.clients[client_id].send_error(format!(r#"Cannot join "{}": no such contest"#, contest_id.0));
            return;
        };

        // Test flags first. Thus we make sure that turns and other actions are
        // not allowed after the time is over.
        contest.test_flags(ctx, now);
        contest.process_client_event(ctx, client_id, now, event);
        contest.post_process(ctx, now);
        contest.last_activity = now;
    }

    fn on_tick(&mut self, ctx: &mut Context, now: Instant) {
        self.gc_old_contests(now);
        self.check_client_connections(ctx, now);
        for contest in self.contests.values_mut() {
            contest.test_flags(ctx, now);
            contest.post_process(ctx, now);
        }
    }

    fn gc_old_contests(&mut self, now: Instant) {
        // Improvement potential. O(1) time GC.
        // Improvement potential. GC unused contests (zero games and/or no players) sooner.
        self.contests.retain(|_, contest| {
            contest.last_activity.duration_since(now) <= CONTEST_GC_INACTIVITY_THRESHOLD
        });
    }

    fn check_client_connections(&mut self, ctx: &mut Context, now: Instant) {
        use HeartbeatOutcome::*;
        ctx.clients.map.retain(|_, client| {
            match client.heart.beat(now) {
                AllGood => true,
                SendBeat => {
                    client.send(BughouseServerEvent::Heartbeat);
                    true
                },
                OtherPartyTemporatyLost => true,
                OtherPartyPermanentlyLost => false,
            }
        });
    }
}

impl Contest {
    fn test_flags(&mut self, ctx: &mut Context, now: Instant) {
        if let Some(GameState{ game_start, ref mut game, .. }) = self.game_state {
            if let Some(game_start) = game_start {
                if game.status() == BughouseGameStatus::Active {
                    let game_now = GameInstant::from_now_game_active(game_start, now);
                    game.test_flag(game_now);
                    if game.status() != BughouseGameStatus::Active {
                        update_score_on_game_over(game, &mut self.scores);
                        let ev = BughouseServerEvent::GameOver {
                            time: game_now,
                            game_status: game.status(),
                            scores: self.scores.clone(),
                        };
                        self.broadcast(ctx, &ev);
                    }
                }
            }
        }
    }

    fn broadcast(&self, ctx: &mut Context, event: &BughouseServerEvent) {
        ctx.hooks.on_server_broadcast_event(event, self.game_state.as_ref(), self.match_history.len() + 1);
        ctx.clients.broadcast(&self.contest_id, event);
    }

    fn process_client_event(
        &mut self, ctx: &mut Context, client_id: ClientId, now: Instant, event: BughouseClientEvent
    ) {
        let result = match event {
            BughouseClientEvent::NewContest{ player_name, .. } => {
                // The contest was created earlier.
                self.join_player(ctx, client_id, now, player_name)
            },
            BughouseClientEvent::Join{ contest_id: _, player_name } => {
                self.join_player(ctx, client_id, now, player_name)
            },
            BughouseClientEvent::SetTeam{ team } => {
                self.process_set_team(ctx, client_id, team)
            },
            BughouseClientEvent::MakeTurn{ turn_input } => {
                self.process_make_turn(ctx, client_id, now, turn_input)
            },
            BughouseClientEvent::CancelPreturn => {
                self.process_cancel_preturn(ctx, client_id)
            },
            BughouseClientEvent::Resign => {
                self.process_resign(ctx, client_id, now)
            },
            BughouseClientEvent::SetReady{ is_ready } => {
                self.process_set_ready(ctx, client_id, is_ready)
            },
            BughouseClientEvent::Leave => {
                self.process_leave(ctx, client_id)
            },
            BughouseClientEvent::UpdateChalkDrawing{ drawing } => {
                self.process_update_chalk_drawing(ctx, client_id, drawing)
            },
            BughouseClientEvent::RequestExport{ format } => {
                self.process_request_export(ctx, client_id, format)
            },
            BughouseClientEvent::ReportPerformace(..) => {
                unreachable!("Contest-independent event must be processed separately");
            },
            BughouseClientEvent::ReportError(..) => {
                unreachable!("Contest-independent event must be processed separately");
            },
            BughouseClientEvent::Heartbeat => {
                unreachable!("Contest-independent event must be processed separately");
            },
        };
        if let Err(err) = result {
            ctx.clients[client_id].send_error(err);
        }
    }

    fn join_player(
        &mut self, ctx: &mut Context, client_id: ClientId, now: Instant, player_name: String
    ) -> EventResult {
        assert!(ctx.clients[client_id].contest_id.is_none());
        assert!(ctx.clients[client_id].player_id.is_none());
        if let Some(ref game_state) = self.game_state {
            let Some(player_id) = self.players.find_by_name(&player_name) else {
                // Improvement potential. Allow joining mid-game in individual mode.
                //   Q. How to balance score in this case? Should we switch to negative numbers?
                return Err("Cannot join: game has already started".to_owned());
            };
            let existing_client_id = ctx.clients.map.iter().find_map(
                |(&id, c)| if c.player_id == Some(player_id) { Some(id) } else { None }
            );
            if let Some(existing_client_id) = existing_client_id {
                if ctx.clients[existing_client_id].heart.healthy() {
                    return Err(format!(r#"Cannot join: client for player "{}" already connected"#, player_name))
                } else {
                    ctx.clients.remove_client(existing_client_id);
                }
            };
            ctx.clients[client_id].contest_id = Some(self.contest_id.clone());
            ctx.clients[client_id].player_id = Some(player_id);
            ctx.clients[client_id].send(self.make_contest_welcome_event());
            // LobbyUpdated should precede GameStarted, because this is how the client gets their
            // team in FixedTeam mode.
            self.send_lobby_updated(ctx);
            ctx.clients[client_id].send(self.make_game_start_event(now, Some(player_id)));
            let chalkboard = game_state.chalkboard.clone();
            ctx.clients[client_id].send(BughouseServerEvent::ChalkboardUpdated{ chalkboard });
            Ok(())
        } else {
            // TODO: Allow to kick players from the lobby when the old client is offline.
            if self.players.find_by_name(&player_name).is_some() {
                return Err(format!("Cannot join: player \"{}\" already exists", player_name));
            }
            if !is_valid_player_name(&player_name) {
                return Err(format!("Invalid player name: \"{}\"", player_name))
            }
            info!(
                "Client {} join contest {} as {}",
                ctx.clients[client_id].logging_id, self.contest_id.0, player_name
            );
            ctx.clients[client_id].contest_id = Some(self.contest_id.clone());
            let player_id = self.players.add_player(Player {
                name: player_name,
                fixed_team: None,
                is_online: true,
                is_ready: false,
            });
            ctx.clients[client_id].player_id = Some(player_id);
            ctx.clients[client_id].send(self.make_contest_welcome_event());
            self.send_lobby_updated(ctx);
            Ok(())
        }
    }

    fn process_set_team(&mut self, ctx: &mut Context, client_id: ClientId, team: Team) -> EventResult {
        let Some(player_id) = ctx.clients[client_id].player_id else {
            return Err("Cannot set team: not joined".to_owned());
        };
        if self.game_state.is_some() {
            return Err("Cannot set team: contest already started".to_owned());
        }
        self.players[player_id].fixed_team = Some(team);
        self.send_lobby_updated(ctx);
        Ok(())
    }

    fn process_make_turn(
        &mut self, ctx: &mut Context, client_id: ClientId, now: Instant, turn_input: TurnInput
    ) -> EventResult {
        let Some(GameState{ ref mut game_start, ref mut game, ref mut preturns, .. }) = self.game_state else {
            return Err("Cannot make turn: no game in progress".to_owned());
        };
        let Some(player_id) = ctx.clients[client_id].player_id else {
            return Err("Cannot make turn: not joined".to_owned());
        };
        let Some(player_bughouse_id) = game.find_player(&self.players[player_id].name) else {
            return Err("Cannot make turn: player does not participate".to_owned());
        };
        let scores = &mut self.scores;
        let mode = game.turn_mode_for_player(player_bughouse_id);
        match mode {
            Ok(TurnMode::Normal) => {
                let mut turns = vec![];
                let game_now = GameInstant::from_now_game_maybe_active(*game_start, now);
                match apply_turn(
                    game_now, player_bughouse_id, turn_input, game, scores
                ) {
                    Ok(turn_event) => {
                        if game_start.is_none() {
                            *game_start = Some(now);
                        }
                        turns.push(turn_event);
                        let opponent_bughouse_id = player_bughouse_id.opponent();
                        if let Some(preturn) = preturns.remove(&opponent_bughouse_id) {
                            if let Ok(preturn_event) = apply_turn(
                                game_now, opponent_bughouse_id, preturn, game, scores
                            ) {
                                turns.push(preturn_event);
                            }
                            // Improvement potential: Report preturn error as well.
                        }
                    },
                    Err(error) => {
                        return Err(format!("Impossible turn: {:?}", error));
                    },
                }
                let ev = BughouseServerEvent::TurnsMade {
                    turns,
                    game_status: game.status(),
                    scores: scores.clone(),
                };
                self.broadcast(ctx, &ev);
                Ok(())
            },
            Ok(TurnMode::Preturn) => {
                match preturns.entry(player_bughouse_id) {
                    hash_map::Entry::Occupied(_) => {
                        Err("Only one premove is supported".to_owned())
                    },
                    hash_map::Entry::Vacant(entry) => {
                        entry.insert(turn_input);
                        Ok(())
                    },
                }
            },
            Err(error) => {
                Err(format!("Impossible turn: {:?}", error))
            },
        }
    }

    fn process_cancel_preturn(&mut self, ctx: &mut Context, client_id: ClientId) -> EventResult {
        let Some(GameState{ ref game, ref mut preturns, .. }) = self.game_state else {
            return Err("Cannot cancel pre-turn: no game in progress".to_owned());
        };
        let Some(player_id) = ctx.clients[client_id].player_id else {
            return Err("Cannot cancel pre-turn: not joined".to_owned());
        };
        let Some(player_bughouse_id) = game.find_player(&self.players[player_id].name) else {
            return Err("Cannot cancel pre-turn: player does not participate".to_owned());
        };
        preturns.remove(&player_bughouse_id);
        Ok(())
    }

    fn process_resign(&mut self, ctx: &mut Context, client_id: ClientId, now: Instant) -> EventResult {
        let Some(GameState{ ref mut game, game_start, .. }) = self.game_state else {
            return Err("Cannot resign: no game in progress".to_owned());
        };
        if game.status() != BughouseGameStatus::Active {
            return Err("Cannot resign: game already over".to_owned());
        }
        let Some(player_id) = ctx.clients[client_id].player_id else {
            return Err("Cannot resign: not joined".to_owned());
        };
        let Some(player_bughouse_id) = game.find_player(&self.players[player_id].name) else {
            return Err("Cannot resign: player does not participate".to_owned());
        };
        let status = BughouseGameStatus::Victory(
            player_bughouse_id.team().opponent(),
            VictoryReason::Resignation
        );
        let scores = &mut self.scores;
        let game_now = GameInstant::from_now_game_maybe_active(game_start, now);
        game.set_status(status, game_now);
        update_score_on_game_over(game, scores);
        let ev = BughouseServerEvent::GameOver {
            time: game_now,
            game_status: status,
            scores: scores.clone(),
        };
        self.broadcast(ctx, &ev);
        Ok(())
    }

    fn process_set_ready(&mut self, ctx: &mut Context, client_id: ClientId, is_ready: bool) -> EventResult {
        let Some(player_id) = ctx.clients[client_id].player_id else {
            return Err("Cannot update readiness: not joined".to_owned());
        };
        if let Some(GameState{ ref game, .. }) = self.game_state {
            if game.status() == BughouseGameStatus::Active {
                return Err("Cannot update readiness: game still in progress".to_owned());
            }
        }
        self.players[player_id].is_ready = is_ready;
        self.send_lobby_updated(ctx);
        Ok(())
    }

    fn process_leave(&mut self, ctx: &mut Context, client_id: ClientId) -> EventResult {
        if let Some(logging_id) = ctx.clients.remove_client(client_id) {
            info!("Client {} left", logging_id);
        }
        // Note. Player will be removed automatically. This has to be the case, otherwise
        // clients disconnected due to a network error would've left abandoned players.
        // Improvement potential. Do we really need this event? Clients are removed when the
        // network channel is closed anyway.
        Ok(())
    }

    fn process_update_chalk_drawing(
        &mut self, ctx: &mut Context, client_id: ClientId, drawing: ChalkDrawing
    ) -> EventResult {
        let Some(GameState{ ref mut chalkboard, ref game, .. }) = self.game_state else {
            return Err("Cannot update chalk drawing: no game in progress".to_owned());
        };
        let Some(player_id) = ctx.clients[client_id].player_id else {
            return Err("Cannot update chalk drawing: not joined".to_owned());
        };
        if game.status() == BughouseGameStatus::Active {
            return Err("Cannot update chalk drawing: can draw only after game is over".to_owned());
        }
        chalkboard.set_drawing(self.players[player_id].name.clone(), drawing);
        let chalkboard = chalkboard.clone();
        self.broadcast(ctx, &BughouseServerEvent::ChalkboardUpdated{ chalkboard });
        Ok(())
    }

    fn process_request_export(
        &self, ctx: &mut Context, client_id: ClientId, format: BughouseExportFormat
    ) -> EventResult {
        let Some(GameState{ ref game, .. }) = self.game_state else {
            return Err("Cannot export: no game in progress".to_owned());
        };
        let all_games = self.match_history.iter().chain(iter::once(game));
        let content = all_games.enumerate().map(|(round, game)| {
            pgn::export_to_bpgn(format, game, round + 1)
        }).join("\n");
        ctx.clients[client_id].send(BughouseServerEvent::GameExportReady{ content });
        Ok(())
    }

    fn post_process(&mut self, ctx: &mut Context, now: Instant) {
        // Improvement potential: Collapse `send_lobby_updated` events generated during one event
        //   processing cycle. Right now there could up to three: one from the event (SetTeam/SetReady),
        //   one from here and one from `self.start_game`.
        //   Idea: Add `ctx.should_update_lobby` bit and check it in the end.
        // TODO: Show lobby players as offline when `!c.heart.is_online()`.
        let active_player_ids: HashSet<_> = ctx.clients.map.values().filter_map(|c| c.player_id).collect();
        if self.game_state.is_none() {
            let mut player_removed = false;
            self.players.map.retain(|id, _| {
                let keep = active_player_ids.contains(id);
                if !keep {
                    player_removed = true;
                }
                keep
            });
            if player_removed {
                self.send_lobby_updated(ctx);
            }
        } else {
            let mut player_online_status_updated = false;
            for (id, player) in self.players.map.iter_mut() {
                let is_online = active_player_ids.contains(id);
                if player.is_online != is_online {
                    player.is_online = is_online;
                    player.is_ready &= is_online;
                    player_online_status_updated = true;
                }
            }
            if player_online_status_updated {
                self.send_lobby_updated(ctx);
            }
        }

        let enough_players = self.players.len() >= TOTAL_PLAYERS;
        let all_ready = self.players.iter().all(|p| p.is_ready);
        let teams_ok = match self.bughouse_rules.teaming {
            Teaming::FixedTeams => {
                let mut has_players_without_team = false;
                let mut num_players_per_team = enum_map!{ _ => 0 };
                for p in self.players.iter() {
                    if let Some(fixed_team) = p.fixed_team {
                        num_players_per_team[fixed_team] += 1;
                    } else {
                        has_players_without_team = true;
                        break;
                    }
                }
                !has_players_without_team && num_players_per_team.values().all(|&n| n == TOTAL_PLAYERS_PER_TEAM)
            },
            Teaming::IndividualMode => true,
        };
        if enough_players && all_ready && teams_ok {
            let mut previous_players = None;
            if let Some(GameState{ ref game, .. }) = self.game_state {
                assert!(game.status() != BughouseGameStatus::Active,
                    "Players must not be allowed to set is_ready flag while the game is active");
                self.match_history.push(game.clone());
                previous_players = Some(game.players().into_iter().map(|p| p.name.clone()).collect());
            }
            self.start_game(ctx, now, previous_players);
        }
    }

    fn start_game(&mut self, ctx: &mut Context, now: Instant, previous_players: Option<Vec<String>>) {
        self.reset_readiness();
        let players_with_boards = self.assign_boards(previous_players);
        let player_map = BughouseGame::make_player_map(players_with_boards.iter().cloned());
        let game = BughouseGame::new(
            self.chess_rules.clone(), self.bughouse_rules.clone(), player_map
        );
        let players_with_boards = players_with_boards.into_iter().map(|(p, board_idx)| {
            ((*p).clone(), board_idx)
        }).collect();
        self.init_scores();
        self.game_state = Some(GameState {
            game,
            game_start: None,
            preturns: HashMap::new(),
            chalkboard: Chalkboard::new(),
            players_with_boards,
        });
        self.broadcast(ctx, &self.make_game_start_event(now, None));
        self.send_lobby_updated(ctx);  // update readiness flags
    }

    fn init_scores(&mut self) {
        match self.bughouse_rules.teaming {
            Teaming::FixedTeams => {},
            Teaming::IndividualMode => {
                assert!(self.scores.per_team.is_empty());
                for p in self.players.iter() {
                    self.scores.per_player.entry(p.name.clone()).or_insert(0);
                }
            }
        }
    }

    fn make_contest_welcome_event(&self) -> BughouseServerEvent {
        BughouseServerEvent::ContestWelcome {
            contest_id: self.contest_id.0.clone(),
            chess_rules: self.chess_rules.clone(),
            bughouse_rules: self.bughouse_rules.clone(),
        }
    }

    // Creates a game start/reconnect event. `player_id` is needed only if reconnecting.
    fn make_game_start_event(&self, now: Instant, player_id: Option<PlayerId>) -> BughouseServerEvent {
        let Some(game_state) = &self.game_state else {
            panic!("Expected ContestState::Game");
        };
        let player_bughouse_id = player_id.and_then(|id| game_state.game.find_player(&self.players[id].name));
        BughouseServerEvent::GameStarted {
            starting_position: game_state.game.starting_position().clone(),
            players: game_state.players_with_boards.clone(),
            time: current_game_time(game_state, now),
            turn_log: game_state.game.turn_log().iter().map(|t| t.trim_for_sending()).collect(),
            preturn: player_bughouse_id.and_then(|id| game_state.preturns.get(&id)).cloned(),
            game_status: game_state.game.status(),
            scores: self.scores.clone(),
        }
    }

    fn send_lobby_updated(&self, ctx: &mut Context) {
        let player_to_send = self.players.iter().cloned().collect();
        self.broadcast(ctx, &BughouseServerEvent::LobbyUpdated {
            players: player_to_send,
        });
    }

    fn reset_readiness(&mut self) {
        self.players.iter_mut().for_each(|p| p.is_ready = false);
    }

    fn assign_boards(&self, previous_players: Option<Vec<String>>)
        -> Vec<(Rc<PlayerInGame>, BughouseBoard)>
    {
        if let Some(assignment) = &self.board_assignment_override {
            return assignment.iter().map(|(name, player_id)| {
                if let Some(player) = self.players.iter().find(|p| &p.name == name) {
                    if let Some(team) = player.fixed_team {
                        assert_eq!(team, player_id.team());
                    }
                }
                let player_in_game = Rc::new(PlayerInGame {
                    name: name.clone(),
                    team: player_id.team(),
                });
                (player_in_game, player_id.board_idx)
            }).collect()
        }

        let mut rng = rand::thread_rng();
        let mut players_per_team = enum_map!{ _ => vec![] };
        match self.bughouse_rules.teaming {
            Teaming::FixedTeams => {
                for p in self.players.iter() {
                    let team = p.fixed_team.unwrap();
                    players_per_team[team].push(Rc::new(PlayerInGame {
                        name: p.name.clone(),
                        team,
                    }));
                }
            },
            Teaming::IndividualMode => {
                // Improvement potential. Instead count the number of times each player participated
                //   and prioritize those who did less than others.
                let mut rng = rand::thread_rng();
                let player_names: HashSet<String> = self.players.iter().map(|p| {
                    assert!(p.fixed_team.is_none());
                    p.name.clone()
                }).collect();
                let high_priority_players: Vec<String>;
                let low_priority_players: Vec<String>;
                if let Some(previous_players) = previous_players {
                    let previous_player_names: HashSet<String> = previous_players.into_iter().collect();
                    high_priority_players = player_names.difference(&previous_player_names).cloned().collect();
                    low_priority_players = previous_player_names.into_iter().collect();
                } else {
                    high_priority_players = Vec::new();
                    low_priority_players = player_names.into_iter().collect();
                }
                let num_high_priority_players = high_priority_players.len();
                let mut current_players: Vec<String> = if num_high_priority_players >= TOTAL_PLAYERS {
                    high_priority_players.choose_multiple(&mut rng, TOTAL_PLAYERS).cloned().collect()
                } else {
                    high_priority_players.into_iter().chain(
                        low_priority_players.choose_multiple(&mut rng, TOTAL_PLAYERS - num_high_priority_players).cloned()
                    ).collect()
                };
                current_players.shuffle(&mut rng);
                for team in Team::iter() {
                    for _ in 0..TOTAL_PLAYERS_PER_TEAM {
                        players_per_team[team].push(Rc::new(PlayerInGame {
                            name: current_players.pop().unwrap().clone(),
                            team,
                        }));
                    }
                }
            },
        }
        players_per_team.into_values().flat_map(|mut team_players| {
            team_players.shuffle(&mut rng);
            let [a, b] = <[Rc<PlayerInGame>; TOTAL_PLAYERS_PER_TEAM]>::try_from(team_players).unwrap();
            vec![
                (a, BughouseBoard::A),
                (b, BughouseBoard::B),
            ]
        }).collect()
    }
}

fn is_valid_player_name(name: &str) -> bool {
    const MAX_NAME_LENGTH: usize = 20;
    if name.is_empty() || name.chars().count() > MAX_NAME_LENGTH {
        false
    } else {
        // Must be in sync with web name input checkers.
        name.chars().all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_')
    }
}

fn current_game_time(game_state: &GameState, now: Instant) -> GameInstant {
    if game_state.game.status() == BughouseGameStatus::Active {
        GameInstant::from_now_game_maybe_active(game_state.game_start, now)
    } else {
        // Normally `clock().total_time_elapsed()` should be the same on all boards. But
        // it could differ in case the of a flag defeat. Consider the following situation:
        // total time is 300 seconds; on board A the game proceeded normally; on board B
        // white didn't make any turns. In this case board A clock would report real wall
        // time (e.g. 300.1s), while board B clock would report exactly 300s, because each
        // player remaining time is always non-negative.
        // Also this example shows that the best approximation to real game time is the
        // minimum of all boards. Everything higher than the minimum is an artifact of not
        // having checked the flags in time.
        let elapsed_since_start = BughouseBoard::iter()
            .map(|board_idx| game_state.game.board(board_idx).clock().total_time_elapsed())
            .min()
            .unwrap();
        GameInstant::from_duration(elapsed_since_start)
    }
}

fn apply_turn(
    game_now: GameInstant, player_bughouse_id: BughousePlayerId, turn_input: TurnInput,
    game: &mut BughouseGame, scores: &mut Scores,
) -> Result<TurnRecord, TurnError> {
    game.try_turn_by_player(player_bughouse_id, &turn_input, TurnMode::Normal, game_now)?;
    if game.status() != BughouseGameStatus::Active {
        update_score_on_game_over(game, scores);
    }
    Ok(game.last_turn_record().unwrap().trim_for_sending())
}

fn update_score_on_game_over(game: &BughouseGame, scores: &mut Scores) {
    let team_scores = match game.status() {
        BughouseGameStatus::Active => panic!("It just so happens that the game here is only mostly over"),
        BughouseGameStatus::Victory(team, _) => {
            let mut s = enum_map!{ _ => 0 };
            s[team] = 2;
            s
        },
        BughouseGameStatus::Draw(_) => enum_map!{ _ => 1 },
    };
    match game.bughouse_rules().teaming {
        Teaming::FixedTeams => {
            assert!(scores.per_player.is_empty());
            for (team, score) in team_scores {
                *scores.per_team.entry(team).or_insert(0) += score;
            }
        },
        Teaming::IndividualMode => {
            assert!(scores.per_team.is_empty());
            for p in game.players() {
                *scores.per_player.entry(p.name.clone()).or_insert(0) += team_scores[p.team];
            }
        }
    }
}

fn process_report_error(ctx: &Context, client_id: ClientId, report: &BughouseClientErrorReport) {
    // TODO: Save errors to DB.
    let logging_id = &ctx.clients[client_id].logging_id;
    match report {
        BughouseClientErrorReport::RustPanic{ panic_info, backtrace } => {
            warn!("Client {logging_id} panicked:\n{panic_info}\nBacktrace: {backtrace}");
        }
        BughouseClientErrorReport::RustError{ message } => {
            warn!("Client {logging_id} experienced Rust error:\n{message}");
        }
        BughouseClientErrorReport::UnknownError{ message } => {
            warn!("Client {logging_id} experienced unknown error:\n{message}");
        }
    }
}
