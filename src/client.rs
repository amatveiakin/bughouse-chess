use std::rc::Rc;
use std::sync::mpsc;

use instant::Instant;

use crate::altered_game::AlteredGame;
use crate::board::TurnError;
use crate::clock::{GameInstant, WallGameTimePair};
use crate::game::{TurnRecord, BughouseGameStatus, BughouseGame};
use crate::grid::Grid;
use crate::event::{BughouseServerEvent, BughouseClientEvent};
use crate::pgn::BughouseExportFormat;
use crate::player::{Player, Team};
use crate::rules::Teaming;
use crate::scores::Scores;


#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TurnCommandError {
    IllegalTurn(TurnError),
    NoGameInProgress,
}

#[derive(Clone, Debug)]
pub enum NotableEvent {
    None,
    GameStarted,
    OpponentTurnMade,
    GameExportReady(String),
}

// TODO: Does it make sense to have CannotApplyEvent instead of panic (that can be caused by many
//   invariant violations in case of bad server behavior anyway).
#[derive(Clone, Debug)]
pub enum EventError {
    ServerReturnedError(String),
    CannotApplyEvent(String),
}

#[derive(Debug)]
pub struct GameState {
    // Starting position.
    pub starting_grid: Grid,
    // Game state including unconfirmed local changes.
    pub alt_game: AlteredGame,
    // Game start time: `None` before first move, non-`None` afterwards.
    pub time_pair: Option<WallGameTimePair>,
}

#[derive(Debug)]
pub struct Contest {
    pub teaming: Teaming,
    // All players including those not participating in the current game.
    pub players: Vec<Player>,
    // Scores from the past matches.
    pub scores: Scores,
    // Whether this client is ready to start a new game.
    pub is_ready: bool,
    // Active game or latest game
    pub game_state: Option<GameState>,
}

pub struct ClientState {
    my_name: String,
    my_team: Option<Team>,
    events_tx: mpsc::Sender<BughouseClientEvent>,
    contest: Option<Contest>,
}

impl ClientState {
    pub fn new(my_name: String, my_team: Option<Team>, events_tx: mpsc::Sender<BughouseClientEvent>) -> Self {
        ClientState {
            my_name,
            my_team,
            events_tx,
            contest: None,
        }
    }

    pub fn my_name(&self) -> &str { &self.my_name }
    pub fn my_team(&self) -> Option<Team> { self.my_team }
    pub fn is_ready(&self) -> Option<bool> { self.contest.as_ref().map(|c| c.is_ready) }
    pub fn contest(&self) -> Option<&Contest> { self.contest.as_ref() }
    pub fn game_state(&self) -> Option<&GameState> { self.contest.as_ref().and_then(|c| c.game_state.as_ref()) }
    pub fn game_state_mut(&mut self) -> Option<&mut GameState> { self.contest.as_mut().and_then(|c| c.game_state.as_mut()) }

    pub fn join(&mut self) {
        self.events_tx.send(BughouseClientEvent::Join {
            player_name: self.my_name.to_owned(),
            team: self.my_team,
        }).unwrap();
    }
    pub fn resign(&mut self) {
        self.events_tx.send(BughouseClientEvent::Resign).unwrap();
    }
    pub fn set_ready(&mut self, is_ready: bool) {
        if let Some(contest) = self.contest.as_mut() {
            contest.is_ready = is_ready;
            self.events_tx.send(BughouseClientEvent::SetReady{ is_ready }).unwrap();
        }
    }
    pub fn leave(&mut self) {
        self.events_tx.send(BughouseClientEvent::Leave).unwrap();
    }
    pub fn reset(&mut self) {
        self.events_tx.send(BughouseClientEvent::Reset).unwrap();
    }
    pub fn request_export(&mut self, format: BughouseExportFormat) {
        self.events_tx.send(BughouseClientEvent::RequestExport{ format }).unwrap();
    }

    pub fn make_turn(&mut self, turn_algebraic: String) -> Result<(), TurnCommandError> {
        let game_state = self.game_state_mut().ok_or(TurnCommandError::NoGameInProgress)?;
        let GameState{ ref mut alt_game, time_pair, .. } = game_state;
        let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, Instant::now());
        if alt_game.status() != BughouseGameStatus::Active {
            Err(TurnCommandError::IllegalTurn(TurnError::GameOver))
        } else if alt_game.can_make_local_turn() {
            alt_game.try_local_turn_algebraic(&turn_algebraic, game_now).map_err(|err| {
                TurnCommandError::IllegalTurn(err)
            })?;
            self.events_tx.send(BughouseClientEvent::MakeTurn {
                turn_algebraic: turn_algebraic
            }).unwrap();
            Ok(())
        } else {
            Err(TurnCommandError::IllegalTurn(TurnError::WrongTurnOrder))
        }
    }

    pub fn cancel_preturn(&mut self) {
        if let Some(GameState{ ref mut alt_game, .. }) = self.game_state_mut() {
            if alt_game.cancel_preturn() {
                self.events_tx.send(BughouseClientEvent::CancelPreturn).unwrap();
            }
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
            ContestStarted{ teaming } => {
                self.contest = Some(Contest {
                    teaming,
                    players: Vec::new(),
                    scores: Scores::new(),
                    is_ready: false,
                    game_state: None,
                });
                Ok(NotableEvent::None)
            },
            LobbyUpdated{ players } => {
                let contest = self.contest.as_mut().ok_or(EventError::CannotApplyEvent("Cannot apply LobbyUpdated: no contest in progress".to_owned()))?;
                // TODO: Fix race condition: is_ready will toggle back and forth if a lobby update
                //   (e.g. is_ready from another player) arrived before is_ready update from this
                //   client reached the server.
                contest.is_ready = players.iter().find(|p| p.name == self.my_name).unwrap().is_ready;
                contest.players = players;
                Ok(NotableEvent::None)
            },
            GameStarted{ chess_rules, bughouse_rules, starting_grid, players, time, turn_log, game_status, scores } => {
                let player_map = BughouseGame::make_player_map(
                    players.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                );
                let time_pair = if turn_log.is_empty() {
                    assert!(time.elapsed_since_start().is_zero());
                    None
                } else {
                    Some(WallGameTimePair::new(Instant::now(), time.approximate()))
                };
                let game = BughouseGame::new_with_grid(
                    chess_rules, bughouse_rules, starting_grid.clone(), player_map
                );
                let my_id = game.find_player(&self.my_name).unwrap();
                let alt_game = AlteredGame::new(my_id, game);
                let contest = self.contest.as_mut().ok_or(EventError::CannotApplyEvent("Cannot apply GameStarted: no contest in progress".to_owned()))?;
                contest.game_state = Some(GameState {
                    starting_grid,
                    alt_game,
                    time_pair,
                });
                for turn in turn_log {
                    self.apply_remote_turn(turn)?;
                }
                self.update_game_status(game_status, time)?;
                self.update_scores(scores)?;
                Ok(NotableEvent::GameStarted)
            },
            TurnsMade{ turns, game_status, scores } => {
                let mut opponent_turn_made = false;
                for turn in turns {
                    let is_opponent_turn = self.apply_remote_turn(turn)?;
                    opponent_turn_made |= is_opponent_turn;
                }
                self.verify_game_status(game_status)?;
                self.update_scores(scores)?;
                Ok(if opponent_turn_made { NotableEvent::OpponentTurnMade } else { NotableEvent::None })
            },
            GameOver{ time, game_status, scores: new_scores } => {
                let contest = self.contest.as_mut().ok_or(EventError::CannotApplyEvent("Cannot apply GameOver: no contest in progress".to_owned()))?;
                let game_state = contest.game_state.as_mut().ok_or(EventError::CannotApplyEvent("Cannot apply GameOver: no game in progress".to_owned()))?;
                assert!(game_state.alt_game.status() == BughouseGameStatus::Active);
                game_state.alt_game.set_status(game_status, time);
                contest.scores = new_scores;
                Ok(NotableEvent::None)
            },
            GameExportReady{ content } => {
                Ok(NotableEvent::GameExportReady(content))
            },
        }
    }

    // Returns if the turn was mady by current player opponent.
    fn apply_remote_turn(&mut self, turn_record: TurnRecord) -> Result<bool, EventError> {
        let TurnRecord{ player_id, turn_algebraic, time } = turn_record;
        let contest = self.contest.as_mut().ok_or(EventError::CannotApplyEvent("Cannot make turn: no contest in progress".to_owned()))?;
        let game_state = contest.game_state.as_mut().ok_or(EventError::CannotApplyEvent("Cannot make turn: no game in progress".to_owned()))?;
        let GameState{ ref mut alt_game, ref mut time_pair, .. } = game_state;
        if alt_game.status() != BughouseGameStatus::Active {
            return Err(EventError::CannotApplyEvent(format!("Cannot make turn {}: game over", turn_algebraic)));
        }
        if time_pair.is_none() {
            // Improvement potential. Sync client/server times better; consider NTP.
            let game_start = GameInstant::game_start().approximate();
            *time_pair = Some(WallGameTimePair::new(Instant::now(), game_start));
        }
        alt_game.apply_remote_turn_algebraic(
            player_id, &turn_algebraic, time
        ).map_err(|err| {
            EventError::CannotApplyEvent(format!("Impossible turn: {}, error: {:?}", turn_algebraic, err))
        })?;
        Ok(player_id == alt_game.my_id().opponent())
    }

    fn verify_game_status(&mut self, game_status: BughouseGameStatus) -> Result<(), EventError> {
        let contest = self.contest.as_mut().ok_or(EventError::CannotApplyEvent("Cannot verify game status: no contest in progress".to_owned()))?;
        let game_state = contest.game_state.as_mut().ok_or(EventError::CannotApplyEvent("Cannot verify game status: no game in progress".to_owned()))?;
        let GameState{ ref mut alt_game, .. } = game_state;
        if game_status != alt_game.status() {
            return Err(EventError::CannotApplyEvent(format!(
                "Expected game status = {:?}, actual = {:?}", game_status, alt_game.status()
            )));
        }
        Ok(())
    }

    fn update_game_status(&mut self, game_status: BughouseGameStatus, game_now: GameInstant)
        -> Result<(), EventError>
    {
        let contest = self.contest.as_mut().ok_or(EventError::CannotApplyEvent("Cannot update game status: no contest in progress".to_owned()))?;
        let game_state = contest.game_state.as_mut().ok_or(EventError::CannotApplyEvent("Cannot update game status: no game in progress".to_owned()))?;
        let GameState{ ref mut alt_game, .. } = game_state;
        if alt_game.status() == BughouseGameStatus::Active {
            if game_status != BughouseGameStatus::Active {
                alt_game.set_status(game_status, game_now);
            }
            Ok(())
        } else {
            self.verify_game_status(game_status)
        }
    }

    fn update_scores(&mut self, new_scores: Scores) -> Result<(), EventError> {
        let contest = self.contest.as_mut().ok_or(EventError::CannotApplyEvent("Cannot update scores: no contest in progress".to_owned()))?;
        contest.scores = new_scores;
        Ok(())
    }
}
