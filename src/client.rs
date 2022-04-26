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
        game: BughouseGame,  // local state; may contain moves not confirmed by the server yet
        game_confirmed: Option<BughouseGame>,  // state from the server, if different from `game`
        game_start: Option<Instant>,
    },
    // TODO: Separate state for `GameOver`.
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
                            game: BughouseGame::new_with_grid(
                                chess_rules, bughouse_rules, starting_grid, player_map
                            ),
                            game_confirmed: None,
                            game_start: None,
                        });
                    },
                    TurnMade{ player_name, turn_algebraic, time, game_status } => {
                        if let ContestState::Game{
                            ref mut game, ref mut game_confirmed, ref mut game_start
                        } = self.contest_state {
                            assert!(game.status() == BughouseGameStatus::Active, "Cannot make turn: game over");
                            if game_start.is_none() {
                                // TODO: Sync client/server times better; consider NTP
                                *game_start = Some(Instant::now());
                            }
                            if player_name == self.my_name {
                                *game = game_confirmed.take().unwrap();
                            }
                            let turn_result = game.try_turn_by_player_from_algebraic(
                                &player_name, &turn_algebraic, time
                            );
                            turn_result.unwrap_or_else(|err| {
                                panic!("Impossible turn: {}, error: {:?}", turn_algebraic, err)
                            });
                            assert_eq!(game_status, game.status());
                        } else {
                            panic!("Cannot make turn: no game in progress")
                        }
                    },
                    GameOver{ time, game_status } => {
                        if let ContestState::Game{ ref mut game, ref mut game_confirmed, .. } = self.contest_state {
                            if let Some(game_confirmed) = game_confirmed.take() {
                                *game = game_confirmed;
                            }
                            assert!(game.status() == BughouseGameStatus::Active);
                            assert!(game_status != BughouseGameStatus::Active);
                            // TODO: Make sure this is synced with flag.
                            game.set_status(game_status, time);
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
            if cmd == "quit" {
                self.events_tx.send(BughouseClientEvent::Leave).unwrap();
                return EventReaction::ExitOk;
            }
            if let ContestState::Game{ ref mut game, ref mut game_confirmed, .. } = self.contest_state {
                if game.player_is_active(&self.my_name).unwrap() {
                    assert!(game_confirmed.is_none());
                    *game_confirmed = Some(game.clone());
                    // Don't try to advance the clock: server is the source of truth for flag defeat.
                    // TODO: Fix time recorded in order to show accurate local time before the server confirmed the move.
                    //   Problem: need to avoid recording flag defeat prematurely.
                    let clock = game.player_board(&self.my_name).unwrap().clock();
                    let turn_start = clock.turn_start().unwrap_or(GameInstant::game_start());
                    let turn_result = game.try_turn_by_player_from_algebraic(
                        &self.my_name, &cmd, turn_start
                    );
                    match turn_result {
                        Ok(_) => {
                            self.events_tx.send(BughouseClientEvent::MakeTurn {
                                turn_algebraic: cmd
                            }).unwrap();
                        },
                        Err(err) => {
                            *game_confirmed = None;
                            // TODO: FIX: Screen is not updated while an error is shown.
                            self.command_error = Some(format!("Illegal turn '{}': {:?}", cmd, err));
                        },
                    }
                } else {
                    self.keyboard_input = cmd;
                }
            } else {
                self.command_error = Some(format!("Unknown command: '{}'", cmd));
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
