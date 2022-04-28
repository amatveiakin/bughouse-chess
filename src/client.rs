use std::rc::Rc;
use std::sync::mpsc;
use std::time::Instant;

use crossterm::{event as term_event};

use crate::clock::{GameInstant};
use crate::game::{BughouseGameStatus, BughouseGame};
use crate::event::{BughouseServerEvent, BughouseClientEvent};
use crate::player::{Player, Team};


#[derive(Debug)]
pub enum IncomingEvent {
    Network(BughouseServerEvent),
    Terminal(term_event::Event),
    Tick,
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[must_use]
pub enum EventReaction {
    Continue,
    ExitOk,
    ExitWithError(String),
}

impl EventReaction {
    pub fn expect_cont(&self) {
        assert!(
            matches!(&self, EventReaction::Continue),
            "Expected the app to continue, found {:?}", &self
        );
    }
}

#[derive(Debug)]
pub enum ContestState {
    Uninitialized,
    Lobby { players: Vec<Player> },
    Game {
        // State as it has been confirmed by the server.
        game_confirmed: BughouseGame,
        // Local turn (algebraic), uncofirmed by the server yet, but displayed on the client.
        local_turn: Option<(String, GameInstant)>,
        // Game start time: `None` before first move, non-`None` afterwards.
        game_start: Option<Instant>,
    },
}

pub fn game_local(my_name: &str, game_confirmed: &BughouseGame, local_turn: &Option<(String, GameInstant)>)
    -> BughouseGame
{
    let mut game = game_confirmed.clone();
    if let Some((turn_algebraic, turn_time)) = local_turn {
        game.try_turn_by_player_from_algebraic(
            my_name, turn_algebraic, *turn_time
        ).unwrap();
    }
    game
}

pub struct ClientState {
    my_name: String,
    my_team: Team,
    events_tx: mpsc::Sender<BughouseClientEvent>,
    contest_state: ContestState,
    command_error: Option<String>,
    keyboard_input: String,
}

impl ClientState {
    pub fn new(my_name: String, my_team: Team, events_tx: mpsc::Sender<BughouseClientEvent>) -> Self {
        ClientState {
            my_name,
            my_team,
            events_tx,
            contest_state: ContestState::Uninitialized,
            command_error: None,
            keyboard_input: String::new(),
        }
    }

    pub fn my_name(&self) -> &str { &self.my_name }
    pub fn contest_state(&self) -> &ContestState { &self.contest_state }
    pub fn command_error(&self) -> &Option<String> { &self.command_error }
    pub fn keyboard_input(&self) -> &String { &self.keyboard_input }

    // Must be called exactly once before calling `apply_event`.
    pub fn join(&mut self) {
        self.events_tx.send(BughouseClientEvent::Join {
            player_name: self.my_name.to_owned(),
            team: self.my_team,
        }).unwrap();
    }

    pub fn apply_event(&mut self, event: IncomingEvent) -> EventReaction {
        let mut command_to_execute = None;
        match event {
            IncomingEvent::Terminal(term_event) => {
                if let term_event::Event::Key(key_event) = term_event {
                    match key_event.code {
                        term_event::KeyCode::Char(ch) => {
                            self.keyboard_input.push(ch);
                        },
                        term_event::KeyCode::Backspace => {
                            self.keyboard_input.pop();
                        },
                        term_event::KeyCode::Enter => {
                            command_to_execute = Some(self.keyboard_input.trim().to_owned());
                            self.keyboard_input.clear();
                        },
                        _ => {},
                    }
                }
            },
            IncomingEvent::Network(net_event) => {
                use BughouseServerEvent::*;
                match net_event {
                    Error{ message } => {
                        return EventReaction::ExitWithError(message);
                    },
                    LobbyUpdated{ players } => {
                        let new_players = players;
                        match self.contest_state {
                            ContestState::Lobby{ ref mut players } => {
                                *players = new_players;
                            },
                            _ => {
                                self.new_contest_state(ContestState::Lobby {
                                    players: new_players
                                });
                            },
                        }
                    },
                    GameStarted{ chess_rules, bughouse_rules, starting_grid, players } => {
                        let player_map = BughouseGame::make_player_map(
                            players.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                        );
                        self.new_contest_state(ContestState::Game {
                            game_confirmed: BughouseGame::new_with_grid(
                                chess_rules, bughouse_rules, starting_grid, player_map
                            ),
                            local_turn: None,
                            game_start: None,
                        });
                    },
                    TurnMade{ player_name, turn_algebraic, time, game_status } => {
                        if let ContestState::Game{
                            ref mut game_confirmed, ref mut local_turn, ref mut game_start
                        } = self.contest_state {
                            assert!(game_confirmed.status() == BughouseGameStatus::Active, "Cannot make turn: game over");
                            if game_start.is_none() {
                                // TODO: Sync client/server times better; consider NTP
                                *game_start = Some(Instant::now());
                            }
                            if player_name == self.my_name {
                                *local_turn = None;
                            }
                            let turn_result = game_confirmed.try_turn_by_player_from_algebraic(
                                &player_name, &turn_algebraic, time
                            );
                            turn_result.unwrap_or_else(|err| {
                                panic!("Impossible turn: {}, error: {:?}", turn_algebraic, err)
                            });
                            assert_eq!(game_status, game_confirmed.status());
                        } else {
                            panic!("Cannot make turn: no game in progress")
                        }
                    },
                    GameOver{ time, game_status } => {
                        if let ContestState::Game{ ref mut game_confirmed, ref mut local_turn, .. }
                            = self.contest_state
                        {
                            *local_turn = None;
                            assert!(game_confirmed.status() == BughouseGameStatus::Active);
                            assert!(game_status != BughouseGameStatus::Active);
                            game_confirmed.set_status(game_status, time);
                        } else {
                            panic!("Cannot record game result: no game in progress")
                        }
                    },
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers repaint, so no additional action is required.
            },
        }

        if let Some(cmd) = command_to_execute {
            self.command_error = None;
            if let Some(cmd) = cmd.strip_prefix('/') {
                match cmd {
                    "quit" => {
                        self.events_tx.send(BughouseClientEvent::Leave).unwrap();
                        return EventReaction::ExitOk;
                    },
                    "resign" => {
                        self.events_tx.send(BughouseClientEvent::Resign).unwrap();
                    },
                    _ => {
                        self.command_error = Some(format!("Unknown command: '{}'", cmd));
                    },
                }
            } else {
                if let ContestState::Game{ ref mut game_confirmed, ref mut local_turn, game_start }
                    = self.contest_state
                {
                    let turn_algebraic = cmd;
                    let game_now = GameInstant::from_maybe_active_game(game_start, Instant::now()).approximate();
                    if game_confirmed.player_is_active(&self.my_name).unwrap() && local_turn.is_none() {
                        let mut game_copy = game_confirmed.clone();
                        // Note. Not calling `test_flag`, because server is the source of truth for flag defeat.
                        let turn_result = game_copy.try_turn_by_player_from_algebraic(
                            &self.my_name, &turn_algebraic, game_now
                        );
                        match turn_result {
                            Ok(_) => {
                                *local_turn = Some((turn_algebraic.clone(), game_now));
                                self.events_tx.send(BughouseClientEvent::MakeTurn {
                                    turn_algebraic: turn_algebraic
                                }).unwrap();
                            },
                            Err(err) => {
                                self.command_error = Some(format!("Illegal turn '{}': {:?}", turn_algebraic, err));
                            },
                        }
                    } else {
                        self.keyboard_input = turn_algebraic;
                    }
                } else {
                    self.command_error = Some("No game in progress".to_owned());
                }
            }
        }
        EventReaction::Continue
    }

    fn new_contest_state(&mut self, contest_state: ContestState) {
        self.contest_state = contest_state;
        self.command_error = None;
        self.keyboard_input.clear();
    }
}
