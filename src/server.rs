use std::collections::HashMap;
use std::ops;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

use enum_map::enum_map;
use rand::prelude::*;

use crate::board::VictoryReason;
use crate::clock::GameInstant;
use crate::game::{BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::event::{BughouseServerEvent, BughouseClientEvent};
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
        game: BughouseGame,
        game_start: Option<Instant>,
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

    pub fn add_client(&mut self, events_tx: mpsc::Sender<BughouseServerEvent>) -> ClientId {
        let client = Client {
            events_tx,
            player_id: None,
        };
        let id = ClientId(self.next_id);
        self.next_id += 1;
        assert!(self.map.insert(id, client).is_none());
        id
    }
    pub fn remove_client(&mut self, id: ClientId) {
        self.map.remove(&id);
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
                        self.core.process_join(&mut clients, client_id, player_name, team);
                    },
                    BughouseClientEvent::MakeTurn{ turn_algebraic } => {
                        self.core.process_make_turn(&mut clients, client_id, now, turn_algebraic);
                    },
                    BughouseClientEvent::Resign => {
                        self.core.process_resign(&mut clients, client_id, now);
                    }
                    BughouseClientEvent::Leave => {
                        self.core.process_leave(&mut clients);
                    },
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers state update, so no additional action is required.
            },
        }

        self.core.post_process(&mut clients);
    }

    #[allow(non_snake_case)]
    pub fn TEST_override_board_assignment(&mut self, assignment: Vec<(String, BughouseBoard)>) {
        assert_eq!(assignment.len(), TOTAL_PLAYERS);
        self.core.board_assignment_override = Some(assignment);
    }
}

impl ServerStateCore {
    fn test_flags(&mut self, clients: &mut ClientsGuard<'_>, now: Instant) {
        if let ContestState::Game{ ref mut game, game_start } = self.contest_state {
            if let Some(game_start) = game_start {
                if game.status() == BughouseGameStatus::Active {
                    let game_now = GameInstant::from_active_game(game_start, now);
                    game.test_flag(game_now);
                    if game.status() != BughouseGameStatus::Active {
                        clients.broadcast(&BughouseServerEvent::GameOver {
                            time: game_now,
                            game_status: game.status(),
                        });
                    }
                }
            }
        }
    }

    fn process_join(
        &mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId,
        player_name: String, team: Team
    ) {
        if let ContestState::Lobby = self.contest_state {
            let mut joined = false;
            if clients[client_id].player_id.is_some() {
                clients[client_id].send_error("Cannot join: already joined".to_owned());
            } else {
                // TODO: Better reconnection:
                //   - Allow to reconnect during the game.
                //   - Remove the player if disconnected while in lobby.
                if let Some(existing_player_id) = self.players.find_by_name(&player_name) {
                    if clients.map.values().find(|c| c.player_id == Some(existing_player_id)).is_some() {
                        clients[client_id].send_error(format!(
                            "Cannot join: client for player \"{}\" already connected", player_name));
                    } else {
                        clients[client_id].player_id = Some(existing_player_id);
                        joined = true;
                    }
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
                        joined = true;
                    }
                }
            }
            if joined {
                // TODO: Use `unwrap_or_clone` when ready: https://github.com/rust-lang/rust/issues/93610
                let player_to_send = self.players.iter().map(|p| (**p).clone()).collect();
                clients.broadcast(&BughouseServerEvent::LobbyUpdated {
                    players: player_to_send,
                });
            }
        } else {
            clients[client_id].send_error("Cannot join: game has already started".to_owned());
        }
    }

    fn process_make_turn(
        &mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant,
        turn_algebraic: String
    ) {
        if let ContestState::Game{ ref mut game, ref mut game_start } = self.contest_state {
            if game_start.is_none() {
                *game_start = Some(now);
            }
            if let Some(player_id) = clients[client_id].player_id {
                let game_now = GameInstant::from_active_game(game_start.unwrap(), now);
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
            } else {
                clients[client_id].send_error("Cannot make turn: not joined".to_owned());
            }
        } else {
            clients[client_id].send_error("Cannot make turn: no game in progress".to_owned());
        }
    }

    fn process_resign(&mut self, clients: &mut ClientsGuard<'_>, client_id: ClientId, now: Instant) {
        if let ContestState::Game{ ref mut game, game_start } = self.contest_state {
            if let Some(player_id) = clients[client_id].player_id {
                let game_now = GameInstant::from_maybe_active_game(game_start, now);
                let status = BughouseGameStatus::Victory(
                    self.players[player_id].team.opponent(),
                    VictoryReason::Resignation
                );
                game.set_status(status, game_now);
                clients.broadcast(&BughouseServerEvent::GameOver {
                    time: game_now,
                    game_status: status,
                });
            } else {
                clients[client_id].send_error("Cannot resign: not joined".to_owned());
            }
        } else {
            clients[client_id].send_error("Cannot resign: no game in progress".to_owned());
        }
    }

    fn process_leave(&mut self, clients: &mut ClientsGuard<'_>) {
        clients.broadcast(&BughouseServerEvent::Error {
            message: "Oh no! Somebody left the party".to_owned(),
        });
    }

    fn post_process(&mut self, clients: &mut ClientsGuard<'_>) {
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
        }
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
