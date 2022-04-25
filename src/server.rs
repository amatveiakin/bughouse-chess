use std::io;
use std::net::TcpStream;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Instant, Duration};

use itertools::Itertools;
use mockstream::SyncMockStream;
use rand::prelude::*;

use crate::clock::{TimeControl, GameInstant};
use crate::coord::{SubjectiveRow};
use crate::game::{BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::event::{BughouseServerEvent, BughouseClientEvent};
use crate::player::{Player, Team};
use crate::rules::{StartingPosition, ChessRules, DropAggression, BughouseRules};
use crate::network;


const TOTAL_PLAYERS: usize = 4;
const TOTAL_PLAYERS_PER_TEAM: usize = 2;

pub trait RWStream : io::Read + io::Write + Send + Sync + 'static {
    fn clone_or_die(&self) -> Self;
}
impl RWStream for TcpStream {
    fn clone_or_die(&self) -> Self { self.try_clone().unwrap() }
}
impl RWStream for SyncMockStream {
    fn clone_or_die(&self) -> Self { self.clone() }
}

pub enum IncomingEvent<S: RWStream> {
    ClientConnected(S),
    ClientEvent(usize, BughouseClientEvent),  // id, event
    Tick,
}

enum ContestState {
    Lobby,
    Game {
        game: BughouseGame,
        game_start: Option<Instant>,
    },
}

// TODO: Rename to `Clients`.
type Players<S> = Vec<(Option<Player>, S)>;

pub struct ServerState<S: RWStream> {
    players: Players<S>,
    contest_state: ContestState,
    new_client_tx: mpsc::Sender<IncomingEvent<S>>,  // should redirect to `apply_event` input
    chess_rules: ChessRules,
    bughouse_rules: BughouseRules,
}

impl<S: RWStream> ServerState<S> {
    pub fn new(new_client_tx: mpsc::Sender<IncomingEvent<S>>) -> Self {
        ServerState {
            players: vec![],
            contest_state: ContestState::Lobby,
            new_client_tx,
            chess_rules: ChessRules {
                starting_position: StartingPosition::FischerRandom,
                time_control: TimeControl{ starting_time: Duration::from_secs(300) },
            },
            bughouse_rules: BughouseRules {
                min_pawn_drop_row: SubjectiveRow::from_one_based(2),
                max_pawn_drop_row: SubjectiveRow::from_one_based(6),
                drop_aggression: DropAggression::NoChessMate,
            },
        }
    }

    // TODO: Better error handling
    pub fn apply_event(&mut self, event: IncomingEvent<S>) {
        let now = Instant::now();
        if let ContestState::Game{ ref mut game, game_start } = self.contest_state {
            if let Some(game_start) = game_start {
                let game_now = GameInstant::new(game_start, now);
                game.test_flag(game_now);
                if game.status() != BughouseGameStatus::Active {
                    broadcast_event(&mut self.players, &BughouseServerEvent::GameOver {
                        time: game_now,
                        game_status: game.status(),
                    }).unwrap();
                    return;
                }
            }
        }

        match event {
            IncomingEvent::ClientConnected(mut stream) => {
                match self.contest_state {
                    ContestState::Lobby => {
                        assert!(self.players.len() < TOTAL_PLAYERS);
                        let player_id = self.players.len();
                        self.players.push((None, stream.clone_or_die()));
                        let tx_new = self.new_client_tx.clone();
                        thread::spawn(move || {
                            loop {
                                let ev = network::parse_obj::<BughouseClientEvent>(
                                    &network::read_str(&mut stream).unwrap()).unwrap();
                                tx_new.send(IncomingEvent::ClientEvent(player_id, ev)).unwrap();
                            }
                        });
                    },
                    ContestState::Game{ .. } => {
                        // TODO: Allow to reconnect
                        network::write_obj(&mut stream, &BughouseServerEvent::Error {
                            message: "Cannot connect: game has already started".to_owned(),
                        }).unwrap();
                    },
                }
            },
            IncomingEvent::ClientEvent(player_id, event) => {
                match event {
                    BughouseClientEvent::Join{ player_name, team } => {
                        if let ContestState::Lobby = self.contest_state {
                            if self.players[player_id].0.is_some() {
                                send_error(&mut self.players, player_id, "Cannot join: already joined".to_owned()).unwrap();
                            } else {
                                // TODO: Check name uniqueness
                                if get_team_players(&self.players, team).count() >= TOTAL_PLAYERS_PER_TEAM {
                                    send_error(&mut self.players, player_id, format!("Cannot join: team {:?} is full", team)).unwrap();
                                } else {
                                    println!("Player {} joined team {:?}", player_name, team);
                                    self.players[player_id].0 = Some(Player {
                                        name: player_name,
                                        team,
                                    });
                                    let player_to_send = self.players.iter().filter_map(|(p, _)| p.clone()).collect_vec();
                                    broadcast_event(&mut self.players, &BughouseServerEvent::LobbyUpdated {
                                        players: player_to_send,
                                    }).unwrap();
                                }
                            }
                        } else {
                            send_error(&mut self.players, player_id, "Cannot join: game has already started".to_owned()).unwrap();
                        }
                    },
                    BughouseClientEvent::MakeTurn{ turn_algebraic } => {
                        if let ContestState::Game{ ref mut game, ref mut game_start } = self.contest_state {
                            if game_start.is_none() {
                                *game_start = Some(now);
                            }
                            let game_now = GameInstant::new(game_start.unwrap(), now);
                            let player_name = self.players[player_id].0.as_ref().unwrap().name.clone();
                            let turn_result = game.try_turn_by_player_from_algebraic(
                                &player_name, &turn_algebraic, game_now
                            );
                            if let Err(error) = turn_result {
                                send_error(&mut self.players, player_id, format!("Impossible turn: {:?}", error)).unwrap();
                            }
                            broadcast_event(&mut self.players, &BughouseServerEvent::TurnMade {
                                player_name: player_name.to_owned(),
                                turn_algebraic,  // TODO: Rewrite turn to a standard form
                                time: game_now,
                                game_status: game.status(),
                            }).unwrap();
                            if game.status() != BughouseGameStatus::Active {
                                return;
                            }
                        } else {
                            send_error(&mut self.players, player_id, "Cannot make turn: no game in progress".to_owned()).unwrap();
                        }
                    },
                    BughouseClientEvent::Leave => {
                        broadcast_event(&mut self.players, &BughouseServerEvent::Error {
                            message: "Oh no! Somebody left the party".to_owned(),
                        }).unwrap();
                    },
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers state update, so no additional action is required.
            },
        }

        if let ContestState::Lobby = self.contest_state {
            assert!(self.players.len() <= TOTAL_PLAYERS);
            if self.players.len() == TOTAL_PLAYERS && self.players.iter().all(|(p, _)| p.is_some()) {
                let players_with_boards = assign_boards(&self.players);
                let player_map = BughouseGame::make_player_map(
                    players_with_boards.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                );
                let game = BughouseGame::new(
                    self.chess_rules.clone(), self.bughouse_rules.clone(), player_map
                );
                let starting_grid = game.board(BughouseBoard::A).grid().clone();
                self.contest_state = ContestState::Game {
                    game,
                    game_start: None,
                };
                broadcast_event(&mut self.players, &BughouseServerEvent::GameStarted {
                    chess_rules: self.chess_rules.clone(),
                    bughouse_rules: self.bughouse_rules.clone(),
                    starting_grid,
                    players: players_with_boards,
                }).unwrap();
            }
        };
    }
}

fn get_team_players<S: RWStream>(players: &Players<S>, team: Team) -> impl Iterator<Item = &Player> {
    players.iter().filter_map(move |(player_or, _)| {
        if let Some(p) = player_or {
            if p.team == team {
                return Some(p);
            }
        }
        None
    })
}

fn send_event<S: RWStream>(players: &mut Players<S>, player_id: usize, event: &BughouseServerEvent) -> io::Result<()> {
    network::write_obj(&mut players[player_id].1, event)
}

fn send_error<S: RWStream>(players: &mut Players<S>, player_id: usize, message: String) -> io::Result<()> {
    send_event(players, player_id, &BughouseServerEvent::Error{ message })
}

fn broadcast_event<S: RWStream>(players: &mut Players<S>, event: &BughouseServerEvent) -> io::Result<()> {
    for (_, stream) in players.iter_mut() {
        network::write_obj(stream, event)?;
    }
    Ok(())
}

fn assign_boards<S: RWStream>(players: &Players<S>) -> Vec<(Player, BughouseBoard)> {
    let mut rng = rand::thread_rng();
    let mut make_team = |team| {
        let mut team_players = get_team_players(players, team).map(|p| p.clone()).collect_vec();
        team_players.shuffle(&mut rng);
        let [a, b] = <[Player; TOTAL_PLAYERS_PER_TEAM]>::try_from(team_players).unwrap();
        vec![
            (a, BughouseBoard::A),
            (b, BughouseBoard::B),
        ]
    };
    [make_team(Team::Red), make_team(Team::Blue)].concat()
}
