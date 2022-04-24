use std::io;
use std::net::{TcpStream, TcpListener};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Instant, Duration};

use itertools::Itertools;
use rand::prelude::*;

use bughouse_chess::*;


const TOTAL_PLAYERS: usize = 4;
const TOTAL_PLAYERS_PER_TEAM: usize = 2;

enum IncomingEvent {
    ClientConnected(TcpStream),
    ClientEvent(usize, BughouseClientEvent),  // id, event
    Tick,
}

enum ServerState {
    Lobby,
    Game {
        game: BughouseGame,
        game_start: Option<Instant>,
    },
}

type Players = Vec<(Option<Player>, TcpStream)>;

fn get_team_players(players: &Players, team: Team) -> impl Iterator<Item = &Player> {
    players.iter().filter_map(move |(player_or, _)| {
        if let Some(p) = player_or {
            if p.team == team {
                return Some(p);
            }
        }
        None
    })
}

fn send_event(players: &mut Players, player_id: usize, event: &BughouseServerEvent) -> io::Result<()> {
    network::write_obj(&mut players[player_id].1, event)
}

fn send_error(players: &mut Players, player_id: usize, message: String) -> io::Result<()> {
    send_event(players, player_id, &BughouseServerEvent::Error{ message })
}

fn broadcast_event(players: &mut Players, event: &BughouseServerEvent) -> io::Result<()> {
    for (_, stream) in players.iter_mut() {
        network::write_obj(stream, event)?;
    }
    Ok(())
}

fn assign_boards(players: &Players) -> Vec<(Player, BughouseBoard)> {
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

// TODO: Better error handling
fn game_main(tx: mpsc::Sender<IncomingEvent>, rx: mpsc::Receiver<IncomingEvent>) {
    let chess_rules: ChessRules = ChessRules {
        starting_position: StartingPosition::FischerRandom,
        time_control: TimeControl{ starting_time: Duration::from_secs(300) },
    };
    let bughouse_rules: BughouseRules = BughouseRules {
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(6),
        drop_aggression: DropAggression::NoChessMate,
    };

    let mut players: Players = Vec::new();
    let mut server_state = ServerState::Lobby;
    for event in rx {
        let now = Instant::now();
        if let ServerState::Game{ ref mut game, game_start } = server_state {
            if let Some(game_start) = game_start {
                let game_now = GameInstant::new(game_start, now);
                game.test_flag(game_now);
                if game.status() != BughouseGameStatus::Active {
                    broadcast_event(&mut players, &BughouseServerEvent::GameOver {
                        time: game_now,
                        game_status: game.status(),
                    }).unwrap();
                    return;
                }
            }
        }

        match event {
            IncomingEvent::ClientConnected(mut stream) => {
                match server_state {
                    ServerState::Lobby => {
                        assert!(players.len() < TOTAL_PLAYERS);
                        let player_id = players.len();
                        players.push((None, stream.try_clone().unwrap()));
                        let tx_new = tx.clone();
                        thread::spawn(move || {
                            loop {
                                let ev = network::parse_obj::<BughouseClientEvent>(
                                    &network::read_str(&mut stream).unwrap()).unwrap();
                                tx_new.send(IncomingEvent::ClientEvent(player_id, ev)).unwrap();
                            }
                        });
                    },
                    ServerState::Game{ .. } => {
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
                        if let ServerState::Lobby = server_state {
                            if players[player_id].0.is_some() {
                                send_error(&mut players, player_id, "Cannot join: already joined".to_owned()).unwrap();
                            } else {
                                // TODO: Check name uniqueness
                                if get_team_players(&players, team).count() >= TOTAL_PLAYERS_PER_TEAM {
                                    send_error(&mut players, player_id, format!("Cannot join: team {:?} is full", team)).unwrap();
                                } else {
                                    println!("Player {} joined team {:?}", player_name, team);
                                    players[player_id].0 = Some(Player {
                                        name: player_name,
                                        team,
                                    });
                                    let player_to_send = players.iter().filter_map(|(p, _)| p.clone()).collect_vec();
                                    broadcast_event(&mut players, &BughouseServerEvent::LobbyUpdated {
                                        players: player_to_send,
                                    }).unwrap();
                                }
                            }
                        } else {
                            send_error(&mut players, player_id, "Cannot join: game has already started".to_owned()).unwrap();
                        }
                    },
                    BughouseClientEvent::MakeTurn{ turn_algebraic } => {
                        if let ServerState::Game{ ref mut game, ref mut game_start } = server_state {
                            if game_start.is_none() {
                                *game_start = Some(now);
                            }
                            let game_now = GameInstant::new(game_start.unwrap(), now);
                            let player_name = players[player_id].0.as_ref().unwrap().name.clone();
                            let turn_result = game.try_turn_by_player_from_algebraic(
                                &player_name, &turn_algebraic, game_now
                            );
                            if let Err(error) = turn_result {
                                send_error(&mut players, player_id, format!("Impossible turn: {:?}", error)).unwrap();
                            }
                            broadcast_event(&mut players, &BughouseServerEvent::TurnMade {
                                player_name: player_name.to_owned(),
                                turn_algebraic,  // TODO: Rewrite turn to a standard form
                                time: game_now,
                                game_status: game.status(),
                            }).unwrap();
                            if game.status() != BughouseGameStatus::Active {
                                return;
                            }
                        } else {
                            send_error(&mut players, player_id, "Cannot make turn: no game in progress".to_owned()).unwrap();
                        }
                    },
                    BughouseClientEvent::Leave => {
                        broadcast_event(&mut players, &BughouseServerEvent::Error {
                            message: "Oh no! Somebody left the party".to_owned(),
                        }).unwrap();
                    },
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers state update, so no additional action is required.
            },
        }

        if let ServerState::Lobby = server_state {
            assert!(players.len() <= TOTAL_PLAYERS);
            if players.len() == TOTAL_PLAYERS && players.iter().all(|(p, _)| p.is_some()) {
                let players_with_boards = assign_boards(&players);
                let player_map = BughouseGame::make_player_map(
                    players_with_boards.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                );
                let game = BughouseGame::new(
                    chess_rules.clone(), bughouse_rules.clone(), player_map
                );
                let starting_grid = game.board(BughouseBoard::A).grid().clone();
                server_state = ServerState::Game {
                    game,
                    game_start: None,
                };
                broadcast_event(&mut players, &BughouseServerEvent::GameStarted {
                    chess_rules: chess_rules.clone(),
                    bughouse_rules: bughouse_rules.clone(),
                    starting_grid,
                    players: players_with_boards,
                }).unwrap();
            }
        };
    }
    panic!("Unexpected end of events stream");
}

pub fn server_main() {
    let (tx, rx) = mpsc::channel();
    let tx_client_connected = tx.clone();
    let tx_tick = tx.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            tx_tick.send(IncomingEvent::Tick).unwrap();
        }
    });
    thread::spawn(move || {
        game_main(tx, rx);
    });

    let listener = TcpListener::bind(("0.0.0.0", network::PORT)).unwrap();
    println!("Listening to connection on {}...", listener.local_addr().unwrap());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                println!("Client connected from {}", stream.peer_addr().unwrap());
                tx_client_connected.send(IncomingEvent::ClientConnected(stream)).unwrap();
            }
            Err(err) => {
                println!("Cannot estanblish connection: {}", err);
            }
        }
    }
    panic!("Unexpected end of TcpListener::incoming");
}
