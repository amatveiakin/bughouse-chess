// Improvement potential. Replace `game.find_player(&self.players[player_id].name)`
//   with a direct mapping (player_id -> player_bughouse_id).

use std::collections::{HashSet, HashMap, hash_map};
use std::iter;
use std::ops;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, mpsc};

use enum_map::enum_map;
use instant::Instant;
use itertools::Itertools;
use log::{info, warn};
use rand::seq::SliceRandom;
use strum::IntoEnumIterator;

use crate::board::{TurnMode, TurnError, TurnInput, VictoryReason};
use crate::clock::GameInstant;
use crate::game::{TurnRecord, BughouseBoard, BughousePlayerId, BughouseGameStatus, BughouseGame};
use crate::grid::Grid;
use crate::event::{BughouseServerEvent, BughouseClientEvent, BughouseClientPerformance, BughouseClientErrorReport};
use crate::pgn::{self, BughouseExportFormat};
use crate::player::{Player, PlayerInGame, Team};
use crate::rules::{Teaming, ChessRules, BughouseRules};
use crate::scores::Scores;
use crate::util::indent_text;
use crate::server_hooks::{ServerHooks, NoopServerHooks};


const TOTAL_PLAYERS: usize = 4;
const TOTAL_PLAYERS_PER_TEAM: usize = 2;

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
    starting_grid: Grid,
    pub players_with_boards: Vec<(PlayerInGame, BughouseBoard)>, // TODO: Extract from `game`
    turn_log: Vec<TurnRecord>,
}


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
    player_id: Option<PlayerId>,
    logging_id: String,
    latest_performance_report: Option<BughouseClientPerformance>,
}

impl Client {
    fn send(&mut self, event: BughouseServerEvent) {
        self.events_tx.send(event).unwrap();
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
        let client = Client {
            events_tx,
            player_id: None,
            logging_id,
            latest_performance_report: None,
        };
        let id = ClientId(self.next_id);
        self.next_id += 1;
        assert!(self.map.insert(id, client).is_none());
        id
    }
    // Returns `logging_id` if the client existed.
    // A client can be removed multiple times, e.g. first on `Leave`, then on network
    // channel closure. This is not an error.
    // Improvement potential. Send an event informing other clients that somebody went
    // offline (for TUI: could use “ϟ” for “disconnected”; there is a plug emoji “🔌”
    // that works much better, but it's not supported by console fonts).
    pub fn remove_client(&mut self, id: ClientId) -> Option<String> {
        let client = self.map.remove(&id);
        if let Some(client) = client {
            if let Some(perf) = &client.latest_performance_report {
                info!(
                    "Client {} info:\nUser agent: {}\nTime zone: {}\nPerformance statistics:\n{}",
                    client.logging_id,
                    perf.user_agent,
                    perf.time_zone,
                    indent_text(&perf.statistics, "    "),
                );
            }
            Some(client.logging_id)
        } else {
            None
        }
    }

    // Sends the event to each client who has joined the game.
    fn broadcast(&mut self, event: &BughouseServerEvent) {
        for Client{events_tx, player_id, ..} in self.map.values() {
            if player_id.is_some() {
                events_tx.send(event.clone()).unwrap();
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

type ClientsGuard<'a> = MutexGuard<'a, Clients>;


struct ServerStateCore {
    chess_rules: ChessRules,
    bughouse_rules: BughouseRules,
    players: Players,
    scores: Scores,
    match_history: Vec<(Grid, BughouseGame)>,  // starting position, final state
    game_state: Option<GameState>,  // active game or latest game
    board_assignment_override: Option<Vec<(String, BughousePlayerId)>>,  // for tests
    hooks: Box<dyn ServerHooks>,
}

// Split state into two parts in order to allow things like:
//   let mut clients = self.clients.lock().unwrap();
//   self.foo(&mut clients);
// which would otherwise make the compiler complain that `self` is borrowed twice.
pub struct ServerState {
    clients: Arc<Mutex<Clients>>,
    core: ServerStateCore,
}

impl ServerState {
    pub fn new(
        clients: Arc<Mutex<Clients>>,
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        hooks: Option<Box<dyn ServerHooks>>,
    ) -> Self {
        ServerState {
            clients,
            core: ServerStateCore {
                chess_rules,
                bughouse_rules,
                players: Players::new(),
                scores: Scores::new(),
                match_history: Vec::new(),
                game_state: None,
                board_assignment_override: None,
                hooks: hooks.unwrap_or(Box::new(NoopServerHooks{})),
            }
        }
    }

    pub fn apply_event(&mut self, event: IncomingEvent) {
        // Use the same timestamp for the entire event processing. Other code reachable
        // from this function should not call `Instant::now()`. Doing so may cause a race
        // condition: e.g. if we check the flag, see that it's ok and then continue to
        // write down a turn which, by that time, becomes illegal because player's time
        // is over.
        let now = Instant::now();

        // Lock clients for the entire duration of the function. This means simpler and
        // more predictable event processing, e.g. it gives a guarantee that all broadcasts
        // from a single `apply_event` reach the same set of clients.
        let mut clients = self.clients.lock().unwrap();

        // Now that we've fixed a `now`, test flags first. Thus we make sure that turns or
        // other actions are not allowed after the time is over.
        self.core.test_flags(&mut clients, now);

        match event {
            IncomingEvent::Network(client_id, event) => {
                if !clients.map.contains_key(&client_id) {
                    // TODO: Should there be an exception for `BughouseClientEvent::ReportError`?
                    // TODO: Improve logging. Consider:
                    //   - include logging_id inside client_id, or
                    //   - keep disconnected clients in the map for some time.
                    warn!("Got an event from disconnected client:\n{event:?}");
                    return;
                }
                match event {
                    BughouseClientEvent::Join{ player_name, team } => {
                        self.core.process_join(&mut clients, client_id, now, player_name, team);
                    },
                    BughouseClientEvent::MakeTurn{ turn_input } => {
                        self.core.process_make_turn(&mut clients, client_id, now, turn_input);
                    },
                    BughouseClientEvent::CancelPreturn => {
                        self.core.process_cancel_preturn(&mut clients, client_id);
                    },
                    BughouseClientEvent::Resign => {
                        self.core.process_resign(&mut clients, client_id, now);
                    },
                    BughouseClientEvent::SetReady{ is_ready } => {
                        self.core.process_set_ready(&mut clients, client_id, is_ready);
                    },
                    BughouseClientEvent::Leave => {
                        self.core.process_leave(&mut clients, client_id);
                    },
                    BughouseClientEvent::Reset => {
                        self.core.process_reset(&mut clients);
                    },
                    BughouseClientEvent::RequestExport{ format } => {
                        self.core.process_request_export(&mut clients, client_id, format);
                    },
                    BughouseClientEvent::ReportPerformace(performance) => {
                        self.core.process_report_performance(&mut clients, client_id, performance);
                    },
                    BughouseClientEvent::ReportError(report) => {
                        self.core.process_report_error(&clients, client_id, report);
                    },
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers state update, so no additional action is required.
            },
        }

        self.core.post_process(&mut clients, now);
    }

    #[allow(non_snake_case)]
    pub fn TEST_override_board_assignment(&mut self, assignment: Vec<(String, BughousePlayerId)>) {
        assert_eq!(assignment.len(), TOTAL_PLAYERS);
        self.core.board_assignment_override = Some(assignment);
    }
}

impl ServerStateCore {
    fn test_flags(&mut self, clients: &mut ClientsGuard<'_>, now: Instant) {
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
                        self.hooks.on_event(&ev, self.game_state.as_ref(), self.match_history.len() + 1);
                        clients.broadcast(&ev);
                    }
                }
            }
        }
    }

    fn process_join(
        &mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant,
        player_name: String, fixed_team: Option<Team>
    ) {
        if self.game_state.is_none() {
            if clients[client_id].player_id.is_some() {
                clients[client_id].send_error("Cannot join: already joined".to_owned());
                return;
            }
            if self.players.find_by_name(&player_name).is_some() {
                clients[client_id].send_error(format!("Cannot join: player \"{}\" already exists", player_name));
                return;
            }
            if fixed_team.is_none() && self.bughouse_rules.teaming == Teaming::FixedTeams {
                clients[client_id].send_error(format!("Cannot join: team is required in fixed teams mode"));
            } else if fixed_team.is_some() && self.bughouse_rules.teaming == Teaming::IndividualMode {
                clients[client_id].send_error(format!("Cannot join: team is not allowed in individual mode"));
            } else if fixed_team.is_some() && self.players.iter().filter(|p| { p.fixed_team == fixed_team }).count() >= TOTAL_PLAYERS_PER_TEAM {
                clients[client_id].send_error(format!("Cannot join: team {:?} is full", fixed_team));
            } else {
                info!("Player {} joined team {:?}", player_name, fixed_team);
                clients[client_id].send(self.make_contest_start_event());
                let player_id = self.players.add_player(Player {
                    name: player_name,
                    fixed_team,
                    is_online: true,
                    is_ready: false,
                });
                clients[client_id].player_id = Some(player_id);
                self.send_lobby_updated(clients);
            }
        } else {
            let Some(existing_player_id) = self.players.find_by_name(&player_name) else {
                // Improvement potential. Allow joining mid-game in individual mode.
                //   Q. How to balance score in this case? Should we switch to negative numbers?
                clients[client_id].send_error("Cannot join: game has already started".to_owned());
                return;
            };
            let current_fixed_team = self.players[existing_player_id].fixed_team;
            if clients.map.values().any(|c| c.player_id == Some(existing_player_id)) {
                clients[client_id].send_error(format!(
                    r#"Cannot join: client for player "{}" already connected"#, player_name));
            } else if current_fixed_team != fixed_team {
                clients[client_id].send_error(format!(
                    r#"Cannot join: player "{}" is in team "{:?}", but connecting as team "{:?}""#,
                    player_name, current_fixed_team, fixed_team
                ));
            } else {
                clients[client_id].player_id = Some(existing_player_id);
                clients[client_id].send(self.make_contest_start_event());
                clients[client_id].send(self.make_game_start_event(now));
            }
        }
    }

    fn process_make_turn(
        &mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant, turn_input: TurnInput
    ) {
        let Some(GameState{ ref mut game_start, ref mut game, ref mut preturns, ref mut turn_log, .. }) = self.game_state else {
            clients[client_id].send_error("Cannot make turn: no game in progress".to_owned());
            return;
        };
        let scores = &mut self.scores;
        let Some(player_id) = clients[client_id].player_id else {
            clients[client_id].send_error("Cannot make turn: not joined".to_owned());
            return;
        };
        let player_bughouse_id = game.find_player(&self.players[player_id].name).unwrap();
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
                        clients[client_id].send_error(format!("Impossible turn: {:?}", error));
                    },
                }
                turn_log.extend_from_slice(&turns);
                let ev = BughouseServerEvent::TurnsMade {
                    turns,
                    game_status: game.status(),
                    scores: scores.clone(),
                };
                self.hooks.on_event(&ev, self.game_state.as_ref(), self.match_history.len() + 1);
                clients.broadcast(&ev);
            },
            Ok(TurnMode::Preturn) => {
                match preturns.entry(player_bughouse_id) {
                    hash_map::Entry::Occupied(_) => {
                        clients[client_id].send_error("Only one premove is supported".to_owned());
                    },
                    hash_map::Entry::Vacant(entry) => {
                        entry.insert(turn_input);
                    },
                }
            },
            Err(error) => {
                clients[client_id].send_error(format!("Impossible turn: {:?}", error));
            },
        }
    }

    fn process_cancel_preturn(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId) {
        let Some(GameState{ ref game, ref mut preturns, .. }) = self.game_state else {
            clients[client_id].send_error("Cannot cancel pre-turn: no game in progress".to_owned());
            return;
        };
        let Some(player_id) = clients[client_id].player_id else {
            clients[client_id].send_error("Cannot cancel pre-turn: not joined".to_owned());
            return;
        };
        let player_bughouse_id = game.find_player(&self.players[player_id].name).unwrap();
        preturns.remove(&player_bughouse_id);
    }

    fn process_resign(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant) {
        let Some(GameState{ ref mut game, game_start, .. }) = self.game_state else {
            clients[client_id].send_error("Cannot resign: no game in progress".to_owned());
            return;
        };
        let scores = &mut self.scores;
        if game.status() != BughouseGameStatus::Active {
            clients[client_id].send_error("Cannot resign: game already over".to_owned());
            return;
        }
        let Some(player_id) = clients[client_id].player_id else {
            clients[client_id].send_error("Cannot resign: not joined".to_owned());
            return;
        };
        let game_now = GameInstant::from_now_game_maybe_active(game_start, now);
        let Some(bughouse_player_id) = game.find_player(&self.players[player_id].name) else {
            clients[client_id].send_error("Cannot resign: player does not participate".to_owned());
            return;
        };
        let status = BughouseGameStatus::Victory(
            bughouse_player_id.team().opponent(),
            VictoryReason::Resignation
        );
        game.set_status(status, game_now);
        update_score_on_game_over(game, scores);
        let ev = BughouseServerEvent::GameOver {
            time: game_now,
            game_status: status,
            scores: scores.clone(),
        };
        self.hooks.on_event(&ev, self.game_state.as_ref(), self.match_history.len() + 1);
        clients.broadcast(&ev);
    }

    fn process_set_ready(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, is_ready: bool) {
        let Some(player_id) = clients[client_id].player_id else {
            clients[client_id].send_error("Cannot update readiness: not joined".to_owned());
            return;
        };
        if let Some(GameState{ ref game, .. }) = self.game_state {
            if game.status() == BughouseGameStatus::Active {
                clients[client_id].send_error("Cannot update readiness: game still in progress".to_owned());
                return;
            }
        }
        self.players[player_id].is_ready = is_ready;
        self.send_lobby_updated(clients);
    }

    fn process_leave(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId) {
        if let Some(logging_id) = clients.remove_client(client_id) {
            info!("Client {} left", logging_id);
        }
        // Note. Player will be removed automatically. This has to be the case, otherwise
        // clients disconnected due to a network error would've left abandoned players.
        // Improvement potential. Do we really need this event? Clients are removed when the
        // network channel is closed anyway.
    }

    fn process_reset(&mut self, clients: &mut ClientsGuard<'_>) {
        self.scores = Scores::new();
        self.match_history = Vec::new();
        self.game_state = None;
        self.reset_readiness();
        let ev = self.make_contest_start_event();
        self.hooks.on_event(&ev, self.game_state.as_ref(), self.match_history.len() + 1);
        clients.broadcast(&ev);
        self.send_lobby_updated(clients);
    }

    fn process_request_export(
        &self, clients: &mut ClientsGuard<'_>, client_id: ClientId, format: BughouseExportFormat)
    {
        let Some(GameState{ ref starting_grid, ref game, .. }) = self.game_state else {
            clients[client_id].send_error("Cannot export: no game in progress".to_owned());
            return;
        };
        // Improvement potential: Replace map lambda with something more elegant.
        let all_games = self.match_history.iter().map(|(grid, game)| (grid, game))
            .chain(iter::once((starting_grid, game)));
        let content = all_games.enumerate().map(|(round, (grid, game))| {
            pgn::export_to_bpgn(format, grid, game, round + 1)
        }).join("\n");
        clients[client_id].send(BughouseServerEvent::GameExportReady{ content });
    }

    fn process_report_performance(
        &self, clients: &mut ClientsGuard<'_>, client_id: ClientId, performance: BughouseClientPerformance)
    {
        clients[client_id].latest_performance_report = Some(performance);
    }

    fn process_report_error(
        &self, clients: &ClientsGuard<'_>, client_id: ClientId, report: BughouseClientErrorReport)
    {
        let logging_id = &clients[client_id].logging_id;
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

    fn post_process(&mut self, clients: &mut ClientsGuard<'_>, now: Instant) {
        let active_player_ids: HashSet<_> = clients.map.values().filter_map(|c| c.player_id).collect();
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
                self.send_lobby_updated(clients);
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
                self.send_lobby_updated(clients);
            }
        }
        if self.players.len() >= TOTAL_PLAYERS && self.players.iter().all(|p| p.is_ready) {
            let mut previous_players = None;
            if let Some(GameState{ ref starting_grid, ref game, .. }) = self.game_state {
                assert!(game.status() != BughouseGameStatus::Active,
                    "Players must not be allowed to set is_ready flag while the game is active");
                self.match_history.push((starting_grid.clone(), game.clone()));
                previous_players = Some(game.players().into_iter().map(|p| p.name.clone()).collect());
            }
            self.start_game(clients, now, previous_players);
        }
    }

    fn start_game(&mut self, clients: &mut ClientsGuard<'_>, now: Instant, previous_players: Option<Vec<String>>) {
        self.reset_readiness();
        let players_with_boards = self.assign_boards(previous_players);
        let player_map = BughouseGame::make_player_map(players_with_boards.iter().cloned());
        let game = BughouseGame::new(
            self.chess_rules.clone(), self.bughouse_rules.clone(), player_map
        );
        let starting_grid = game.board(BughouseBoard::A).grid().clone();
        let players_with_boards = players_with_boards.into_iter().map(|(p, board_idx)| {
            ((*p).clone(), board_idx)
        }).collect();
        self.init_scores();
        self.game_state = Some(GameState {
            game,
            game_start: None,
            preturns: HashMap::new(),
            starting_grid,
            players_with_boards,
            turn_log: vec![],
        });
        let ev = self.make_game_start_event(now);
        self.hooks.on_event(&ev, self.game_state.as_ref(), self.match_history.len() + 1);
        clients.broadcast(&ev);
        self.send_lobby_updated(clients);  // update readiness flags
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

    fn make_contest_start_event(&self) -> BughouseServerEvent {
        BughouseServerEvent::ContestStarted {
            teaming: self.bughouse_rules.teaming,
        }
    }

    fn make_game_start_event(&self, now: Instant) -> BughouseServerEvent {
        let Some(game_state) = &self.game_state else {
            panic!("Expected ContestState::Game");
        };
        let time = GameInstant::from_now_game_maybe_active(game_state.game_start, now);
        BughouseServerEvent::GameStarted {
            chess_rules: self.chess_rules.clone(),
            bughouse_rules: self.bughouse_rules.clone(),
            starting_grid: game_state.starting_grid.clone(),
            players: game_state.players_with_boards.clone(),
            time,
            turn_log: game_state.turn_log.clone(),
            game_status: game_state.game.status(),
            scores: self.scores.clone(),
        }
    }

    fn send_lobby_updated(&mut self, clients: &mut ClientsGuard<'_>) {
        let player_to_send = self.players.iter().cloned().collect();
        let ev = BughouseServerEvent::LobbyUpdated {
            players: player_to_send,
        };
        self.hooks.on_event(&ev, self.game_state.as_ref(), self.match_history.len() + 1);
        clients.broadcast(&ev);
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
