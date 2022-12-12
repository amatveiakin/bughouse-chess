use crate::board::{Turn, TurnInput, TurnMove, TurnDrop, TurnMode, TurnError};
use crate::clock::GameInstant;
use crate::coord::{SubjectiveRow, Coord};
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
#[derive(Debug)]
pub struct AlteredGame {
    // All local actions are assumed to be made on behalf of this player.
    my_id: BughouseParticipantId,
    // State as it has been confirmed by the server.
    game_confirmed: BughouseGame,
    // Local turn:
    //   - if TurnMode::Normal: a turn not confirmed by the server yet, but displayed on
    //       the client; always a valid turn for the `game_confirmed`;
    //   - if TurnMode::Preturn: a preturn.
    local_turn: Option<(TurnInput, TurnMode, GameInstant)>,
    // Drag&drop state if making turn by mouse or touch.
    piece_drag: Option<PieceDrag>,
}

impl AlteredGame {
    pub fn new(my_id: BughouseParticipantId, game_confirmed: BughouseGame) -> Self {
        AlteredGame {
            my_id,
            game_confirmed,
            local_turn: None,
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
                if player_id == my_player_id {
                    self.local_turn = None;
                } else if let Some((turn_input, turn_mode, turn_time)) = self.local_turn.take() {
                    assert_eq!(turn_mode, TurnMode::Preturn);
                    // Note: this will now be executed as a local turn and rejected if invalid.
                    _ = self.try_local_turn(turn_input, turn_time);
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
    pub fn game_confirmed(&self) -> &BughouseGame { &self.game_confirmed }

    pub fn local_game(&self) -> BughouseGame {
        let mut game = self.game_confirmed.clone();
        if let Some((ref turn_input, mode, turn_time)) = self.local_turn {
            let BughouseParticipantId::Player(my_player_id) = self.my_id else {
                panic!("Only an active player can make moves");
            };
            // Note. Not calling `test_flag`, because only server records flag defeat.
            game.try_turn_by_player(my_player_id, turn_input, mode, turn_time).unwrap();
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

    pub fn can_make_local_turn(&self) -> bool {
        self.local_turn.is_none()
    }

    pub fn try_local_turn(&mut self, turn_input: TurnInput, time: GameInstant)
        -> Result<TurnMode, TurnError>
    {
        let BughouseParticipantId::Player(my_player_id) = self.my_id else {
            return Err(TurnError::NotPlayer);
        };
        let mut game_copy = self.game_confirmed.clone();
        let mode = game_copy.turn_mode_for_player(my_player_id)?;
        game_copy.try_turn_by_player(my_player_id, &turn_input, mode, time)?;
        self.local_turn = Some((turn_input, mode, time));
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
        if matches!(self.local_turn, Some((_, TurnMode::Preturn, _))) {
            self.local_turn = None;
            true
        } else {
            false
        }
    }
    pub fn reset_local_changes(&mut self) {
        self.local_turn = None;
        self.piece_drag = None;
    }
}
