use std::rc::Rc;
use std::sync::mpsc;

use instant::Instant;

use crate::board::{TurnError};
use crate::clock::{GameInstant, WallGameTimePair};
use crate::game::{BughouseGameStatus, BughouseGame};
use crate::event::{TurnMadeEvent, BughouseServerEvent, BughouseClientEvent};
use crate::player::{Player, Team};


#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TurnCommandError {
    IllegalTurn(TurnError),
    NoGameInProgress,
}

#[derive(Clone, Debug)]
pub enum NotableEvent {
    None,
    GameStarted,
}

#[derive(Clone, Debug)]
pub enum EventError {
    ServerReturnedError(String),
    CannotApplyEvent(String),
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
        time_pair: Option<WallGameTimePair>,
    },
}

pub fn game_local(my_name: &str, game_confirmed: &BughouseGame, local_turn: &Option<(String, GameInstant)>)
    -> BughouseGame
{
    let mut game = game_confirmed.clone();
    if let Some((turn_algebraic, turn_time)) = local_turn {
        game.try_turn_by_player_from_algebraic(my_name, turn_algebraic, *turn_time).unwrap();
    }
    game
}

pub struct ClientState {
    my_name: String,
    my_team: Team,
    events_tx: mpsc::Sender<BughouseClientEvent>,
    contest_state: ContestState,
}

impl ClientState {
    pub fn new(my_name: String, my_team: Team, events_tx: mpsc::Sender<BughouseClientEvent>) -> Self {
        ClientState {
            my_name,
            my_team,
            events_tx,
            contest_state: ContestState::Uninitialized,
        }
    }

    pub fn my_name(&self) -> &str { &self.my_name }
    pub fn contest_state(&self) -> &ContestState { &self.contest_state }

    pub fn join(&mut self) {
        self.events_tx.send(BughouseClientEvent::Join {
            player_name: self.my_name.to_owned(),
            team: self.my_team,
        }).unwrap();
    }
    pub fn resign(&mut self) {
        self.events_tx.send(BughouseClientEvent::Resign).unwrap();
    }
    pub fn new_game(&mut self) {
        self.events_tx.send(BughouseClientEvent::NewGame).unwrap();
    }
    pub fn leave(&mut self) {
        self.events_tx.send(BughouseClientEvent::Leave).unwrap();
    }

    pub fn make_turn(&mut self, turn_algebraic: String) -> Result<(), TurnCommandError> {
        if let ContestState::Game{ ref mut game_confirmed, ref mut local_turn, time_pair }
            = self.contest_state
        {
            let game_now = GameInstant::from_pair_game_maybe_active(time_pair, Instant::now());
            if game_confirmed.player_is_active(&self.my_name).unwrap() && local_turn.is_none() {
                let mut game_copy = game_confirmed.clone();
                // Note. Not calling `test_flag`, because server is the source of truth for flag defeat.
                game_copy.try_turn_by_player_from_algebraic(
                    &self.my_name, &turn_algebraic, game_now
                ).map_err(|err| {
                    TurnCommandError::IllegalTurn(err)
                })?;
                *local_turn = Some((turn_algebraic.clone(), game_now));
                self.events_tx.send(BughouseClientEvent::MakeTurn {
                    turn_algebraic: turn_algebraic
                }).unwrap();
                Ok(())
            } else {
                Err(TurnCommandError::IllegalTurn(TurnError::WrongTurnOrder))
            }
        } else {
            Err(TurnCommandError::NoGameInProgress)
        }
    }

    // TODO: This is becoming a weird mixture of rendering `ContestState` AND processing `NotableEvent`s.
    //   Consider whether `ClientState` should become a processor of turning events from server
    //   into more digestable client events that client implementations work on (while never reading
    //   the state directly.
    pub fn process_server_event(&mut self, event: BughouseServerEvent) -> Result<NotableEvent, EventError> {
        use BughouseServerEvent::*;
        match event {
            Error{ message } => {
                Err(EventError::ServerReturnedError(format!("Got error from server: {}", message)))
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
                Ok(NotableEvent::None)
            },
            GameStarted{ chess_rules, bughouse_rules, starting_grid, players, time, turn_log } => {
                let player_map = BughouseGame::make_player_map(
                    players.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                );
                let time_pair = if turn_log.is_empty() {
                    assert!(time.elapsed_since_start().is_zero());
                    None
                } else {
                    Some(WallGameTimePair::new(Instant::now(), time.approximate()))
                };
                self.new_contest_state(ContestState::Game {
                    game_confirmed: BughouseGame::new_with_grid(
                        chess_rules, bughouse_rules, starting_grid, player_map
                    ),
                    local_turn: None,
                    time_pair,
                });
                for event in turn_log {
                    self.apply_turn(event)?;
                }
                Ok(NotableEvent::GameStarted)
            },
            TurnMade(event) => {
                self.apply_turn(event)?;
                Ok(NotableEvent::None)
            },
            GameOver{ time, game_status } => {
                if let ContestState::Game{ ref mut game_confirmed, ref mut local_turn, .. }
                    = self.contest_state
                {
                    *local_turn = None;
                    assert!(game_confirmed.status() == BughouseGameStatus::Active);
                    assert!(game_status != BughouseGameStatus::Active);
                    game_confirmed.set_status(game_status, time);
                    Ok(NotableEvent::None)
                } else {
                    Err(EventError::CannotApplyEvent("Cannot record game result: no game in progress".to_owned()))
                }
            },
        }
    }

    fn apply_turn(&mut self, event: TurnMadeEvent) -> Result<(), EventError> {
        let TurnMadeEvent{ player_name, turn_algebraic, time, game_status } = event;
        if let ContestState::Game{
            ref mut game_confirmed, ref mut local_turn, ref mut time_pair
        } = self.contest_state {
            // TODO: Simply ignore turns after game over.
            assert!(game_confirmed.status() == BughouseGameStatus::Active, "Cannot make turn: game over");
            if time_pair.is_none() {
                // Improvement potential. Sync client/server times better; consider NTP.
                let game_start = GameInstant::game_start().approximate();
                *time_pair = Some(WallGameTimePair::new(Instant::now(), game_start));
            }
            if player_name == self.my_name {
                *local_turn = None;
            }
            let turn_result = game_confirmed.try_turn_by_player_from_algebraic(
                &player_name, &turn_algebraic, time
            );
            turn_result.map_err(|err| {
                EventError::CannotApplyEvent(format!("Impossible turn: {}, error: {:?}", turn_algebraic, err))
            })?;
            if game_status != game_confirmed.status() {
                return Err(EventError::CannotApplyEvent(format!(
                    "Expected game status = {:?}, actual = {:?}", game_status, game_confirmed.status()
                )))
            }
            Ok(())
        } else {
            Err(EventError::CannotApplyEvent("Cannot make turn: no game in progress".to_owned()))
        }
    }

    // TODO: Is this function needed? (maybe always produce a NotableEvent here)
    fn new_contest_state(&mut self, contest_state: ContestState) {
        self.contest_state = contest_state;
    }
}
