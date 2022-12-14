use std::cmp;

use crate::board::{Turn, TurnInput, TurnMove, TurnDrop, TurnMode, TurnError};
use crate::clock::GameInstant;
use crate::coord::{SubjectiveRow, Coord};
use crate::display::Perspective;
use crate::game::{BughouseParticipantId, BughousePlayerId, BughouseGameStatus, BughouseGame};
use crate::piece::{CastleDirection, PieceKind};


#[derive(Clone, Copy, Debug)]
pub enum PieceDragStart {
    Board(Coord),
    Reserve(PieceKind),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceDragError {
    DragForbidden,
    DragAlreadyStarted,
    NoDragInProgress,
    DragNoLongerPossible,
    PieceNotFound,
    Cancelled,
}

#[derive(Clone, Copy, Debug)]
pub enum PieceDragSource {
    Defunct,  // in case dragged piece was captured by opponent
    Board(Coord),
    Reserve,
}

#[derive(Clone, Debug)]
pub struct PieceDrag {
    pub piece_kind: PieceKind,
    pub source: PieceDragSource,
    pub dest: Option<Coord>,
}

// In online multiplayer: game with local changes not (yet) confirmed by the server.
//
// Only one preturn is allowed, but it's possible to have two unconfirmed local turns: one
// normal and one preturn.
#[derive(Debug)]
pub struct AlteredGame {
    // All local actions are assumed to be made on behalf of this player.
    my_id: BughouseParticipantId,
    // State as it has been confirmed by the server.
    game_confirmed: BughouseGame,
    // Local turn (TurnMode::Normal) not confirmed by the server yet, but displayed on the
    // client. Always a valid turn for the `game_confirmed`.
    local_turn: Option<(TurnInput, GameInstant)>,
    // Local preturn (TurnMode::Preturn). Executed after `local_turn` if the latter exists.
    local_preturn: Option<(TurnInput, GameInstant)>,
    // Drag&drop state if making turn by mouse or touch.
    piece_drag: Option<PieceDrag>,
}

impl AlteredGame {
    pub fn new(my_id: BughouseParticipantId, game_confirmed: BughouseGame) -> Self {
        AlteredGame {
            my_id,
            game_confirmed,
            local_turn: None,
            local_preturn: None,
            piece_drag: None,
        }
    }

    // Status returned by this function may differ from `local_game()` status.
    // This function should be used as the source of truth when showing game status to the
    // user, as it's possible that the final status from the server will be different, e.g.
    // if the game ended earlier on the other board.
    pub fn status(&self) -> BughouseGameStatus {
        self.game_confirmed.status()
    }

    pub fn set_status(&mut self, status: BughouseGameStatus, time: GameInstant) {
        assert!(status != BughouseGameStatus::Active);
        self.reset_local_changes();
        self.game_confirmed.set_status(status, time);
    }

    pub fn apply_remote_turn_algebraic(
        &mut self, player_id: BughousePlayerId, turn_algebraic: &str, time: GameInstant)
        -> Result<Turn, TurnError>
    {
        let turn_input = TurnInput::Algebraic(turn_algebraic.to_owned());
        let turn = self.game_confirmed.try_turn_by_player(
            player_id, &turn_input, TurnMode::Normal, time
        )?;

        if self.game_confirmed.status() != BughouseGameStatus::Active {
            self.reset_local_changes();
        } else if let BughouseParticipantId::Player(my_player_id) = self.my_id {
            if player_id.board_idx == my_player_id.board_idx {
                // Something on our board has changed. Time to figure out which local turns to keep.
                // An alternative solution would've been to annotate each turn with turn number and
                // remove all obsolete turns. But that's boring. So here we go...
                if player_id == my_player_id {
                    // The server confirmed a turn made by us. If a local turn exists, that must be
                    // it. Thus the local copy must be discarded.
                    //
                    // It's possible that in addition to a local turn there exists a local preturn.
                    // This is fine, and it should still be treated as a preturn until the opponent
                    // makes a turn.
                    //
                    // However it is not possible to have just a local preturn (without the normal
                    // turn) in this context: the preturn would never be confirmed until the opponent
                    // makes their turn.
                    //
                    // On the other hand, it is possible to receive a turn from the server while not
                    // having any local turns. This happens if preturn cancellation didn't make it
                    // to the server in time.
                    if self.local_turn.is_none() {
                        assert!(self.local_preturn.is_none());
                    }
                    self.local_turn = None;
                } else {
                    // A server sent us a turn made by our opponent. Therefore we cannot have a
                    // pending local turn - that would be out of order. But we could have a pending
                    // preturn. This preturn, if it exists, should be re-applied as a normal turn
                    // to the updated board. As the result, it could be cancelled (either because
                    // the position has changed or simply because it is now a subject to stricter
                    // verification).
                    //
                    // Note that if the preturn is still valid, we would normally get it back
                    // together with the opponent's turn in the same `TurnsMade` event. But we
                    // cannot count on this: it is possible that our preturn has not reached the
                    // server by the time the server processed opponent's turn.
                    assert!(self.local_turn.is_none());
                    if let Some((turn_input, original_time)) = self.local_preturn.take() {
                        // Make sure we don't go back in time by making a turn before a confirmed
                        // opponent's turn. An alternative solution would be to allow only `Approximate`
                        // time measurement everywhere in the client code (including tests).
                        let my_board = self.game_confirmed.board(player_id.board_idx);
                        let opponent_turn_time = my_board.clock().total_time_elapsed();
                        let t = cmp::max(original_time.elapsed_since_start(), opponent_turn_time);
                        let turn_time = GameInstant::from_duration(t).set_measurement(original_time.measurement());
                        // A-a-and we are ready to reapply the preturn. As a normal turn now.
                        // Ignore any errors: it's normal for preturns to fail.
                        _ = self.try_local_turn(turn_input, turn_time);
                    }
                }
            }
        }

        if let Some(ref mut drag) = self.piece_drag {
            let BughouseParticipantId::Player(my_player_id) = self.my_id else {
                panic!("Only an active player can drag pieces");
            };
            if let PieceDragSource::Board(coord) = drag.source {
                let board = self.game_confirmed.board(my_player_id.board_idx);
                if board.grid()[coord].map(|piece| piece.force) != Some(my_player_id.force) {
                    // Dragged piece was captured by opponent.
                    drag.source = PieceDragSource::Defunct;
                }
            }
        }
        Ok(turn)
    }

    pub fn my_id(&self) -> BughouseParticipantId { self.my_id }
    pub fn perspective(&self) -> Perspective { Perspective::for_force(self.my_id.visual_force()) }
    pub fn game_confirmed(&self) -> &BughouseGame { &self.game_confirmed }

    pub fn local_game(&self) -> BughouseGame {
        let mut game = self.game_confirmed.clone();
        if let Some((ref turn_input, turn_time)) = self.local_turn {
            self.apply_local_turn(&mut game, turn_input, TurnMode::Normal, turn_time);
        }
        if let Some((ref turn_input, turn_time)) = self.local_preturn {
            self.apply_local_turn(&mut game, turn_input, TurnMode::Preturn, turn_time);
        }
        if let Some(ref drag) = self.piece_drag {
            let BughouseParticipantId::Player(my_player_id) = self.my_id else {
                panic!("Only an active player can drag pieces");
            };
            let board = game.board_mut(my_player_id.board_idx);
            match drag.source {
                PieceDragSource::Defunct => {},
                PieceDragSource::Board(coord) => {
                    let piece = board.grid_mut()[coord].take().unwrap();
                    assert_eq!(piece.force, my_player_id.force);
                    assert_eq!(piece.kind, drag.piece_kind);
                },
                PieceDragSource::Reserve => {
                    let reserve = board.reserve_mut(my_player_id.force);
                    assert!(reserve[drag.piece_kind] > 0);
                    reserve[drag.piece_kind] -= 1;
                }
            }
        }
        game
    }

    pub fn try_local_turn(&mut self, turn_input: TurnInput, time: GameInstant)
        -> Result<TurnMode, TurnError>
    {
        let BughouseParticipantId::Player(my_player_id) = self.my_id else {
            return Err(TurnError::NotPlayer);
        };
        if self.local_preturn.is_some() {
            return Err(TurnError::PreturnLimitReached);
        }
        let mut game = self.local_game();
        let mode = game.turn_mode_for_player(my_player_id)?;
        game.try_turn_by_player(my_player_id, &turn_input, mode, time)?;
        match mode {
            TurnMode::Normal => self.local_turn = Some((turn_input, time)),
            TurnMode::Preturn => self.local_preturn = Some((turn_input, time)),
        };
        self.piece_drag = None;
        Ok(mode)
    }

    pub fn piece_drag_state(&self) -> &Option<PieceDrag> {
        &self.piece_drag
    }

    pub fn start_drag_piece(&mut self, start: PieceDragStart) -> Result<(), PieceDragError> {
        let BughouseParticipantId::Player(my_player_id) = self.my_id else {
            return Err(PieceDragError::DragForbidden);
        };
        if self.piece_drag.is_some() {
            return Err(PieceDragError::DragAlreadyStarted);
        }
        let (piece_kind, source) = match start {
            PieceDragStart::Board(coord) => {
                let game = self.local_game();
                let board = game.board(my_player_id.board_idx);
                let piece = board.grid()[coord].ok_or(PieceDragError::PieceNotFound)?;
                (piece.kind, PieceDragSource::Board(coord))
            },
            PieceDragStart::Reserve(piece_kind) => {
                (piece_kind, PieceDragSource::Reserve)
            },
        };
        self.piece_drag = Some(PieceDrag {
            piece_kind,
            source,
            dest: None,
        });
        Ok(())
    }

    pub fn drag_over_piece(&mut self, dest: Option<Coord>) -> Result<(), PieceDragError> {
        if let Some(ref mut drag) = self.piece_drag {
            drag.dest = dest;
        } else {
            return Err(PieceDragError::NoDragInProgress);
        }
        Ok(())
    }

    pub fn abort_drag_piece(&mut self) {
        self.piece_drag = None;
    }

    // Stop drag and returns algebraic turn on success.
    pub fn drag_piece_drop(&mut self, dest_coord: Coord, promote_to: PieceKind)
        -> Result<TurnInput, PieceDragError>
    {
        let BughouseParticipantId::Player(my_player_id) = self.my_id else {
            return Err(PieceDragError::DragForbidden);
        };
        let drag = self.piece_drag.as_ref().ok_or(PieceDragError::NoDragInProgress)?;
        let piece_kind = drag.piece_kind;
        let source = drag.source;
        self.piece_drag = None;

        match source {
            PieceDragSource::Defunct => {
                Err(PieceDragError::DragNoLongerPossible)
            },
            PieceDragSource::Board(source_coord) => {
                use PieceKind::*;
                if source_coord == dest_coord {
                    return Err(PieceDragError::Cancelled);
                }
                let force = my_player_id.force;
                let first_row = SubjectiveRow::from_one_based(1).to_row(force);
                let last_row = SubjectiveRow::from_one_based(8).to_row(force);
                let d_col = dest_coord.col - source_coord.col;
                let is_castling =
                    piece_kind == King &&
                    (d_col.abs() >= 2) &&
                    (source_coord.row == first_row && dest_coord.row == first_row)
                ;
                let is_promotion = piece_kind == Pawn && dest_coord.row == last_row;
                if is_castling {
                    use CastleDirection::*;
                    let dir = if d_col > 0 { HSide } else { ASide };
                    Ok(TurnInput::DragDrop(Turn::Castle(dir)))
                } else {
                    Ok(TurnInput::DragDrop(Turn::Move(TurnMove {
                        from: source_coord,
                        to: dest_coord,
                        promote_to: if is_promotion { Some(promote_to) } else { None },
                    })))
                }
            },
            PieceDragSource::Reserve => {
                Ok(TurnInput::DragDrop(Turn::Drop(TurnDrop {
                    piece_kind,
                    to: dest_coord
                })))
            }
        }
    }

    pub fn cancel_preturn(&mut self) -> bool {
        self.local_preturn.take().is_some()
    }
    pub fn reset_local_changes(&mut self) {
        self.local_turn = None;
        self.local_preturn = None;
        self.piece_drag = None;
    }

    fn apply_local_turn(
        &self, game: &mut BughouseGame, turn_input: &TurnInput, mode: TurnMode, turn_time: GameInstant
    ) {
        let BughouseParticipantId::Player(my_player_id) = self.my_id else {
            panic!("Only an active player can make moves");
        };
        // Note. Not calling `test_flag`, because only server records flag defeat.
        // Note. Safe to unwrap: turn correctness (according to the `mode`) has already been verified.
        game.try_turn_by_player(my_player_id, turn_input, mode, turn_time).unwrap();
    }
}
