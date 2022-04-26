use std::collections::{hash_map, HashMap};
use std::ops;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use enum_map::enum_map;
use rand::prelude::*;

use crate::clock::GameInstant;
use crate::game::{BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::event::{BughouseServerEvent, BughouseClientEvent};
use crate::player::Player;
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
        game: BughouseGame,
        game_start: Option<Instant>,
    },
}


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PlayerId(usize);

struct Players {
    map: HashMap<PlayerId, Rc<Player>>,
}

impl Players {
    fn new() -> Self { Self{ map: HashMap::new() } }
    fn len(&self) -> usize { self.map.len() }
    fn iter(&self) -> impl Iterator<Item = &Rc<Player>> { self.map.values() }
    fn add_player(&mut self, player: Rc<Player>) -> PlayerId {
        loop {
            let id = PlayerId(rand::random());
            match self.map.entry(id) {
                hash_map::Entry::Occupied(_) => {},
                hash_map::Entry::Vacant(e) => {
                    e.insert(player);
                    return id;
                }
            }
        }
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
}

impl Clients {
    pub fn new() -> Self { Self{ map: HashMap::new() } }

    pub fn add_client(&mut self, events_tx: mpsc::Sender<BughouseServerEvent>) -> ClientId {
        let client = Client {
            events_tx,
            player_id: None,
        };
        loop {
            let id = ClientId(rand::random());
            match self.map.entry(id) {
                hash_map::Entry::Occupied(_) => {},
                hash_map::Entry::Vacant(e) => {
                    e.insert(client);
                    return id;
                }
            }
        }
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


pub struct ServerState {
    clients: Arc<Mutex<Clients>>,
    players: Players,
    contest_state: ContestState,
    chess_rules: ChessRules,
    bughouse_rules: BughouseRules,
    board_assignment_override: Option<Vec<(String, BughouseBoard)>>,  // for tests
}

impl ServerState {
    pub fn new(
        clients: Arc<Mutex<Clients>>,
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules
    ) -> Self {
        ServerState {
            clients,
            chess_rules,
            bughouse_rules,
            players: Players::new(),
            contest_state: ContestState::Lobby,
            board_assignment_override: None,
        }
    }

    // TODO: Better error handling
    pub fn apply_event(&mut self, event: IncomingEvent) {
        let now = Instant::now();
        let mut clients = self.clients.lock().unwrap();

        if let ContestState::Game{ ref mut game, game_start } = self.contest_state {
            if let Some(game_start) = game_start {
                let game_now = GameInstant::new(game_start, now);
                game.test_flag(game_now);
                if game.status() != BughouseGameStatus::Active {
                    clients.broadcast(&BughouseServerEvent::GameOver {
                        time: game_now,
                        game_status: game.status(),
                    });
                    return;
                }
            }
        }

        match event {
            IncomingEvent::Network(client_id, event) => {
                match event {
                    BughouseClientEvent::Join{ player_name, team } => {
                        if let ContestState::Lobby = self.contest_state {
                            if clients[client_id].player_id.is_some() {
                                clients[client_id].send_error("Cannot join: already joined".to_owned());
                            } else {
                                // TODO: Check name uniqueness
                                if self.players.iter().filter(|p| { p.team == team }).count() >= TOTAL_PLAYERS_PER_TEAM {
                                    clients[client_id].send_error(format!("Cannot join: team {:?} is full", team));
                                } else {
                                    println!("Player {} joined team {:?}", player_name, team);
                                    let player_id = self.players.add_player(Rc::new(Player {
                                        name: player_name,
                                        team,
                                    }));
                                    clients[client_id].player_id = Some(player_id);
                                    // TODO: Use `unwrap_or_clone` when ready: https://github.com/rust-lang/rust/issues/93610
                                    let player_to_send = self.players.iter().map(|p| (**p).clone()).collect();
                                    clients.broadcast(&BughouseServerEvent::LobbyUpdated {
                                        players: player_to_send,
                                    });
                                }
                            }
                        } else {
                            clients[client_id].send_error("Cannot join: game has already started".to_owned());
                        }
                    },
                    BughouseClientEvent::MakeTurn{ turn_algebraic } => {
                        if let ContestState::Game{ ref mut game, ref mut game_start } = self.contest_state {
                            if game_start.is_none() {
                                *game_start = Some(now);
                            }
                            if let Some(player_id) = clients[client_id].player_id {
                                let game_now = GameInstant::new(game_start.unwrap(), now);
                                let player_name = self.players[player_id].name.clone();
                                let turn_result = game.try_turn_by_player_from_algebraic(
                                    &player_name, &turn_algebraic, game_now
                                );
                                if let Err(error) = turn_result {
                                    clients[client_id].send_error(format!("Impossible turn: {:?}", error));
                                }
                                clients.broadcast(&BughouseServerEvent::TurnMade {
                                    player_name: player_name.to_owned(),
                                    turn_algebraic,  // TODO: Rewrite turn to a standard form
                                    time: game_now,
                                    game_status: game.status(),
                                });
                                if game.status() != BughouseGameStatus::Active {
                                    return;
                                }
                            } else {
                                clients[client_id].send_error("Cannot make turn: not joined".to_owned());
                            }
                        } else {
                            clients[client_id].send_error("Cannot make turn: no game in progress".to_owned());
                        }
                    },
                    BughouseClientEvent::Leave => {
                        clients.broadcast(&BughouseServerEvent::Error {
                            message: "Oh no! Somebody left the party".to_owned(),
                        });
                    },
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers state update, so no additional action is required.
            },
        }

        if let ContestState::Lobby = self.contest_state {
            assert!(self.players.len() <= TOTAL_PLAYERS);
            if self.players.len() == TOTAL_PLAYERS {
                let players_with_boards = self.assign_boards(self.players.iter());
                let player_map = BughouseGame::make_player_map(players_with_boards.iter().cloned());
                let game = BughouseGame::new(
                    self.chess_rules.clone(), self.bughouse_rules.clone(), player_map
                );
                let starting_grid = game.board(BughouseBoard::A).grid().clone();
                self.contest_state = ContestState::Game {
                    game,
                    game_start: None,
                };
                // TODO: Use `unwrap_or_clone` when ready: https://github.com/rust-lang/rust/issues/93610
                let player_to_send = players_with_boards.into_iter().map(|(p, board_idx)| {
                    ((*p).clone(), board_idx)
                }).collect();
                clients.broadcast(&BughouseServerEvent::GameStarted {
                    chess_rules: self.chess_rules.clone(),
                    bughouse_rules: self.bughouse_rules.clone(),
                    starting_grid,
                    players: player_to_send,
                });
            }
        };
    }

    #[allow(non_snake_case)]
    pub fn TEST_override_board_assignment(&mut self, assignment: Vec<(String, BughouseBoard)>) {
        assert_eq!(assignment.len(), TOTAL_PLAYERS);
        self.board_assignment_override = Some(assignment);
    }

    fn assign_boards<'a>(&self, players: impl Iterator<Item = &'a Rc<Player>>)
        -> Vec<(Rc<Player>, BughouseBoard)>
    {
        if let Some(assignment) = &self.board_assignment_override {
            let players_by_name: HashMap<_, _> = players.map(|p| (&p.name, p)).collect();
            assignment.iter().map(|(name, board_idx)| {
                (Rc::clone(players_by_name[name]), *board_idx)
            }).collect()
        } else {
            let mut rng = rand::thread_rng();
            let mut players_per_team = enum_map!{ _ => vec![] };
            for p in players {
                players_per_team[p.team].push(Rc::clone(p));
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
