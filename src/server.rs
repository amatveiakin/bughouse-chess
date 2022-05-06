use std::collections::{HashSet, HashMap};
use std::ops;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, mpsc};

use enum_map::{EnumMap, enum_map};
use instant::Instant;
use itertools::Itertools;
use rand::prelude::*;

use crate::board::VictoryReason;
use crate::clock::GameInstant;
use crate::game::{BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::grid::Grid;
use crate::event::{TurnMadeEvent, BughouseServerEvent, BughouseClientEvent};
use crate::player::{Player, Team};
use crate::rules::{ChessRules, BughouseRules};


const TOTAL_PLAYERS: usize = 4;
const TOTAL_PLAYERS_PER_TEAM: usize = 2;

#[derive(Debug)]
pub enum IncomingEvent {
    Network(ClientId, BughouseClientEvent),
    Tick,
}

#[derive(Debug)]
enum ContestState {
    Lobby,
    Game {
        scores: EnumMap<Team, u32>,  // victory scored as 2:0, draw is 1:1
        game: BughouseGame,
        game_start: Option<Instant>,
        starting_grid: Grid,
        players_with_boards: Vec<(Player, BughouseBoard)>,
        turn_log: Vec<TurnMadeEvent>,
    },
}


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PlayerId(usize);

struct Players {
    map: HashMap<PlayerId, Rc<Player>>,
    next_id: usize,
}

impl Players {
    fn new() -> Self { Self{ map: HashMap::new(), next_id: 1 } }
    fn len(&self) -> usize { self.map.len() }
    fn iter(&self) -> impl Iterator<Item = &Rc<Player>> { self.map.values() }
    fn find_by_name(&self, name: &str) -> Option<PlayerId> {
        self.map.iter().find_map(|(id, p)| if p.name == name { Some(*id) } else { None })
    }
    fn add_player(&mut self, player: Rc<Player>) -> PlayerId {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        assert!(self.map.insert(id, player).is_none());
        id
    }
}

impl ops::Index<PlayerId> for Players {
    type Output = Rc<Player>;
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
        self.map.remove(&id).map(|client| client.logging_id)
    }

    fn broadcast(&mut self, event: &BughouseServerEvent) {
        for (_, Client{events_tx, ..}) in &self.map {
            events_tx.send(event.clone()).unwrap();
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
    players: Players,
    contest_state: ContestState,
    chess_rules: ChessRules,
    bughouse_rules: BughouseRules,
    board_assignment_override: Option<Vec<(String, BughouseBoard)>>,  // for tests
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
        bughouse_rules: BughouseRules
    ) -> Self {
        ServerState {
            clients,
            core: ServerStateCore {
                chess_rules,
                bughouse_rules,
                players: Players::new(),
                contest_state: ContestState::Lobby,
                board_assignment_override: None,
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
                match event {
                    BughouseClientEvent::Join{ player_name, team } => {
                        self.core.process_join(&mut clients, client_id, now, player_name, team);
                    },
                    BughouseClientEvent::MakeTurn{ turn_algebraic } => {
                        self.core.process_make_turn(&mut clients, client_id, now, turn_algebraic);
                    },
                    BughouseClientEvent::Resign => {
                        self.core.process_resign(&mut clients, client_id, now);
                    },
                    BughouseClientEvent::NextGame => {
                        self.core.process_next_game(&mut clients, client_id, now);
                    },
                    BughouseClientEvent::Leave => {
                        self.core.process_leave(&mut clients, client_id);
                    },
                    BughouseClientEvent::Reset => {
                        self.core.process_reset();
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
    pub fn TEST_override_board_assignment(&mut self, assignment: Vec<(String, BughouseBoard)>) {
        assert_eq!(assignment.len(), TOTAL_PLAYERS);
        self.core.board_assignment_override = Some(assignment);
    }
}

impl ServerStateCore {
    fn test_flags(&mut self, clients: &mut ClientsGuard<'_>, now: Instant) {
        if let ContestState::Game{ ref mut game, game_start, ref mut scores, .. } = self.contest_state {
            if let Some(game_start) = game_start {
                if game.status() == BughouseGameStatus::Active {
                    let game_now = GameInstant::from_now_game_active(game_start, now);
                    game.test_flag(game_now);
                    if game.status() != BughouseGameStatus::Active {
                        update_score_on_game_over(game.status(), scores);
                        clients.broadcast(&BughouseServerEvent::GameOver {
                            scores: scores.clone().into_iter().collect(),
                            time: game_now,
                            game_status: game.status(),
                        });
                    }
                }
            }
        }
    }

    fn process_join(
        &mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant,
        player_name: String, team: Team
    ) {
        match self.contest_state {
            ContestState::Lobby => {
                if clients[client_id].player_id.is_some() {
                    clients[client_id].send_error("Cannot join: already joined".to_owned());
                } else {
                    if let Some(_) = self.players.find_by_name(&player_name) {
                        clients[client_id].send_error(format!("Cannot join: player \"{}\" already exists", player_name));
                    } else {
                        if self.players.iter().filter(|p| { p.team == team }).count() >= TOTAL_PLAYERS_PER_TEAM {
                            clients[client_id].send_error(format!("Cannot join: team {:?} is full", team));
                        } else {
                            println!("Player {} joined team {:?}", player_name, team);
                            let player_id = self.players.add_player(Rc::new(Player {
                                name: player_name,
                                team,
                            }));
                            clients[client_id].player_id = Some(player_id);
                            self.send_lobby_updated(clients);
                        }
                    }
                }
            },
            ContestState::Game{ .. } => {
                if let Some(existing_player_id) = self.players.find_by_name(&player_name) {
                    let current_team = self.players[existing_player_id].team;
                    if clients.map.values().find(|c| c.player_id == Some(existing_player_id)).is_some() {
                        clients[client_id].send_error(format!(
                            r#"Cannot join: client for player "{}" already connected"#, player_name));
                    } else if current_team != team {
                        clients[client_id].send_error(format!(
                            r#"Cannot join: player "{}" is in team "{:?}", but connecting as team "{:?}""#,
                            player_name, current_team, team
                        ));
                    } else {
                        clients[client_id].player_id = Some(existing_player_id);
                        clients[client_id].send(self.make_game_start_event(now));
                    }
                } else {
                    clients[client_id].send_error("Cannot join: game has already started".to_owned());
                }
            }
        }
    }

    fn process_make_turn(
        &mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant,
        turn_algebraic: String
    ) {
        if let ContestState::Game{ ref mut game, ref mut game_start, ref mut turn_log, ref mut scores, .. }
            = self.contest_state
        {
            if game_start.is_none() {
                *game_start = Some(now);
            }
            if let Some(player_id) = clients[client_id].player_id {
                let game_now = GameInstant::from_now_game_active(game_start.unwrap(), now);
                let player_name = self.players[player_id].name.clone();
                let turn_result = game.try_turn_by_player_from_algebraic(
                    &player_name, &turn_algebraic, game_now
                );
                if let Err(error) = turn_result {
                    clients[client_id].send_error(format!("Impossible turn: {:?}", error));
                    return;
                }
                if game.status() != BughouseGameStatus::Active {
                    update_score_on_game_over(game.status(), scores);
                }
                let turn_made_event = TurnMadeEvent {
                    player_name: player_name.to_owned(),
                    turn_algebraic,  // TODO: Rewrite turn to a standard form
                    time: game_now,
                    game_status: game.status(),
                    scores: scores.clone().into_iter().collect(),
                };
                turn_log.push(turn_made_event.clone());
                clients.broadcast(&BughouseServerEvent::TurnMade(turn_made_event));
            } else {
                clients[client_id].send_error("Cannot make turn: not joined".to_owned());
            }
        } else {
            clients[client_id].send_error("Cannot make turn: no game in progress".to_owned());
        }
    }

    fn process_resign(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant) {
        if let ContestState::Game{ ref mut game, game_start, ref mut scores, .. } = self.contest_state {
            if game.status() != BughouseGameStatus::Active {
                clients[client_id].send_error("Cannot resign: game already over".to_owned());
                return;
            }
            if let Some(player_id) = clients[client_id].player_id {
                let game_now = GameInstant::from_now_game_maybe_active(game_start, now);
                let status = BughouseGameStatus::Victory(
                    self.players[player_id].team.opponent(),
                    VictoryReason::Resignation
                );
                game.set_status(status, game_now);
                update_score_on_game_over(status, scores);
                clients.broadcast(&BughouseServerEvent::GameOver {
                    time: game_now,
                    game_status: status,
                    scores: scores.clone().into_iter().collect(),
                });
            } else {
                clients[client_id].send_error("Cannot resign: not joined".to_owned());
            }
        } else {
            clients[client_id].send_error("Cannot resign: no game in progress".to_owned());
        }
    }

    fn process_next_game(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant) {
        if let ContestState::Game{ ref game, ref scores, .. } = self.contest_state {
            if game.status() == BughouseGameStatus::Active {
                clients[client_id].send_error("Cannot start next game: game still in progress".to_owned());
            } else {
                let players = game.players();
                let scores = scores.clone();
                self.start_game(clients, now, players.into_iter(), scores);
            }
        } else {
            clients[client_id].send_error("Cannot start next game: game not assembled".to_owned());
        }
    }

    fn process_leave(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId) {
        if let Some(logging_id) = clients.remove_client(client_id) {
            println!("Client {} disconnected", logging_id);
        }
        // Note. Player will be removed automatically. This has to be the case, otherwise
        // clients disconnected due to a network error would've left abandoned players.
    }

    fn process_reset(&mut self) {
        self.contest_state = ContestState::Lobby;
    }

    fn post_process(&mut self, clients: &mut ClientsGuard<'_>, now: Instant) {
        if let ContestState::Lobby = self.contest_state {
            let active_player_ids: HashSet<_> = clients.map.values().filter_map(|c| c.player_id).collect();
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
            assert!(self.players.len() <= TOTAL_PLAYERS);
            if self.players.len() == TOTAL_PLAYERS {
                let players = self.players.iter().cloned().collect_vec();
                let scores = enum_map!{ _ => 0 };
                self.start_game(clients, now, players.into_iter(), scores);
            }
        }
    }

    fn start_game(
        &mut self, clients: &mut ClientsGuard<'_>, now: Instant,
        players: impl Iterator<Item = Rc<Player>>, scores: EnumMap<Team, u32>
    ) {
        let players_with_boards = self.assign_boards(players);
        let player_map = BughouseGame::make_player_map(players_with_boards.iter().cloned());
        let game = BughouseGame::new(
            self.chess_rules.clone(), self.bughouse_rules.clone(), player_map
        );
        let starting_grid = game.board(BughouseBoard::A).grid().clone();
        let players_with_boards = players_with_boards.into_iter().map(|(p, board_idx)| {
            ((*p).clone(), board_idx)
        }).collect();
        self.contest_state = ContestState::Game {
            scores,
            game,
            game_start: None,
            starting_grid,
            players_with_boards,
            turn_log: vec![],
        };
        clients.broadcast(&self.make_game_start_event(now));
    }

    fn make_game_start_event(&self, now: Instant) -> BughouseServerEvent {
        // Improvement potential: Pass `ContestState` from above: it should already be
        //   unpacked where the function is called.
        if let ContestState::Game{ scores, game_start, starting_grid, players_with_boards, turn_log, .. }
            = &self.contest_state
        {
            let time = GameInstant::from_now_game_maybe_active(*game_start, now);
            BughouseServerEvent::GameStarted {
                chess_rules: self.chess_rules.clone(),
                bughouse_rules: self.bughouse_rules.clone(),
                scores: scores.clone().into_iter().collect(),
                starting_grid: starting_grid.clone(),
                players: players_with_boards.clone(),
                time,
                turn_log: turn_log.clone(),
            }
        } else {
            panic!("Expected ContestState::Game");
        }
    }

    fn send_lobby_updated(&self, clients: &mut ClientsGuard<'_>) {
        let player_to_send = self.players.iter().map(|p| (**p).clone()).collect();
        clients.broadcast(&BughouseServerEvent::LobbyUpdated {
            players: player_to_send,
        });
    }

    fn assign_boards(&self, players: impl Iterator<Item = Rc<Player>>)
        -> Vec<(Rc<Player>, BughouseBoard)>
    {
        if let Some(assignment) = &self.board_assignment_override {
            let players_by_name: HashMap<_, _> = players.map(|p| (p.name.clone(), p)).collect();
            assignment.iter().map(|(name, board_idx)| {
                (Rc::clone(&players_by_name[name]), *board_idx)
            }).collect()
        } else {
            let mut rng = rand::thread_rng();
            let mut players_per_team = enum_map!{ _ => vec![] };
            for p in players {
                players_per_team[p.team].push(p);
            }
            players_per_team.into_values().map(|mut team_players| {
                team_players.shuffle(&mut rng);
                let [a, b] = <[Rc<Player>; TOTAL_PLAYERS_PER_TEAM]>::try_from(team_players).unwrap();
                vec![
                    (a, BughouseBoard::A),
                    (b, BughouseBoard::B),
                ]
            }).flatten().collect()
        }
    }
}

fn update_score_on_game_over(status: BughouseGameStatus, scores: &mut EnumMap<Team, u32>) {
    match status {
        BughouseGameStatus::Active => {
            panic!("It just so happens that the game here is only mostly over");
        },
        BughouseGameStatus::Victory(team, _) => {
            scores[team] += 2;
        },
        BughouseGameStatus::Draw => {
            for v in scores.values_mut() {
                *v += 1;
            }
        },
    }
}
