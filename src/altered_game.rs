use crate::board::{Turn, TurnMove, TurnDrop, TurnMode, TurnError};
use crate::clock::GameInstant;
use crate::coord::{SubjectiveRow, Col, Coord};
use crate::game::{BughousePlayerId, BughouseGameStatus, BughouseGame};
use crate::piece::{CastleDirection, PieceKind};


#[derive(Clone, Copy, Debug)]
pub enum PieceDragStart {
    Board(Coord),
    Reserve(PieceKind),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceDragError {
    DragAlreadyStarted,
    NoDragInProgress,
    DragNoLongerPossible,
    PieceNotFound,
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
    my_id: BughousePlayerId,
    // State as it has been confirmed by the server.
    game_confirmed: BughouseGame,
    // Latest turn made by the opponent on this board, used for highlighting.
    latest_opponent_turn: Option<Turn>,
    // Local turn:
    //   - if TurnMode::Normal: a turn not confirmed by the server yet, but displayed on
    //       the client; always a valid turn for the `game_confirmed`;
    //   - if TurnMode::Preturn: a preturn.
    local_turn: Option<(Turn, TurnMode, GameInstant)>,
    // Drag&drop state if making turn by mouse or touch.
    piece_drag: Option<PieceDrag>,
}

impl AlteredGame {
    pub fn new(my_id: BughousePlayerId, game_confirmed: BughouseGame) -> Self {
        AlteredGame {
            my_id,
            game_confirmed,
            latest_opponent_turn: None,
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
        if player_id.board_idx == self.my_id.board_idx {
            self.local_turn = None;
        }
        let turn = self.game_confirmed.try_turn_algebraic_by_player(
            player_id, &turn_algebraic, TurnMode::Normal, time
        )?;
        if player_id == self.my_id.opponent() {
            self.latest_opponent_turn = Some(turn);
        }
        if self.game_confirmed.status() != BughouseGameStatus::Active {
            self.reset_local_changes();
        }
        if let Some(ref mut drag) = self.piece_drag {
            if let PieceDragSource::Board(coord) = drag.source {
                let board = self.game_confirmed.board(self.my_id.board_idx);
                if board.grid()[coord].map(|piece| piece.force) != Some(self.my_id.force) {
                    // Dragged piece was captured by opponent.
                    drag.source = PieceDragSource::Defunct;
                }
            }
        }
        Ok(turn)
    }

    pub fn my_id(&self) -> BughousePlayerId { self.my_id }
    pub fn game_confirmed(&self) -> &BughouseGame { &self.game_confirmed }

    pub fn opponent_turn_highlight(&self) -> Option<Turn> {
        let show_highlight =
            self.game_confirmed.player_is_active(self.my_id) &&
            self.local_turn.is_none();
        if show_highlight { self.latest_opponent_turn } else { None }
    }
    pub fn preturn_highlight(&self) -> Option<Turn> {
        if let Some((turn, TurnMode::Preturn, _)) = self.local_turn { Some(turn) } else { None }
    }

    pub fn local_game(&self) -> BughouseGame {
        let mut game = self.game_confirmed.clone();
        if let Some((turn, mode, turn_time)) = self.local_turn {
            // Note. Not calling `test_flag`, because only server records flag defeat.
            game.try_turn_by_player(self.my_id, turn, mode, turn_time).unwrap();
        }
        if let Some(ref drag) = self.piece_drag {
            let board = game.board_mut(self.my_id.board_idx);
            match drag.source {
                PieceDragSource::Defunct => {},
                PieceDragSource::Board(coord) => {
                    let piece = board.grid_mut()[coord].take().unwrap();
                    assert_eq!(piece.force, self.my_id.force);
                    assert_eq!(piece.kind, drag.piece_kind);
                },
                PieceDragSource::Reserve => {
                    let reserve = board.reserve_mut(self.my_id.force);
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

    pub fn try_local_turn_algebraic(&mut self, turn_algebraic: &str, time: GameInstant)
        -> Result<(), TurnError>
    {
        let mut game_copy = self.game_confirmed.clone();
        let mode = game_copy.turn_mode_for_player(self.my_id)?;
        let turn = game_copy.try_turn_algebraic_by_player(self.my_id, turn_algebraic, mode, time)?;
        self.local_turn = Some((turn, mode, time));
        self.piece_drag = None;
        Ok(())
    }

    pub fn piece_drag_state(&self) -> &Option<PieceDrag> {
        &self.piece_drag
    }

    pub fn start_drag_piece(&mut self, start: PieceDragStart) -> Result<(), PieceDragError> {
        if self.piece_drag.is_some() {
            return Err(PieceDragError::DragAlreadyStarted);
        }
        let (piece_kind, source) = match start {
            PieceDragStart::Board(coord) => {
                let game = self.local_game();
                let board = game.board(self.my_id.board_idx);
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
        -> Result<Turn, PieceDragError>
    {
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
                let force = self.my_id.force;
                let game = self.local_game();
                let board = game.board(self.my_id.board_idx);
                let castling_rights = board.castling_rights()[force];
                let first_row = SubjectiveRow::from_one_based(1).to_row(force);
                let last_row = SubjectiveRow::from_one_based(8).to_row(force);
                let d_col = dest_coord.col - source_coord.col;
                let d_col_abs = d_col.abs();
                let corner_castling = if let Some(piece_to) = board.grid()[dest_coord] {
                    piece_to.force == force &&
                    piece_to.kind == PieceKind::Rook &&
                    ((dest_coord.col == Col::A && castling_rights[CastleDirection::ASide] == Some(Col::A)) ||
                     (dest_coord.col == Col::H && castling_rights[CastleDirection::HSide] == Some(Col::H)))
                } else {
                    false
                };
                // Castling rules: drag the king at least two squares in the rook direction
                // or onto a rook standing in the corner next to the king. That later is
                // required for Fischer random when king starts on b or g.
                // Note. The rule is made as strict as possible (e.g. not just "move king onto
                // a friedly piece in the castling direction") in order to disambiguate premoves
                // better.
                // Improvement potential: Allow premove capturing a rook on a1/a8/h1/h8.
                let is_castling =
                    piece_kind == King &&
                    (d_col_abs >= 2 || (d_col_abs == 1 && corner_castling)) &&
                    (source_coord.row == first_row && dest_coord.row == first_row)
                ;
                let is_promotion = piece_kind == Pawn && dest_coord.row == last_row;
                if is_castling {
                    use CastleDirection::*;
                    let dir = if d_col > 0 { HSide } else { ASide };
                    Ok(Turn::Castle(dir))
                } else {
                    Ok(Turn::Move(TurnMove {
                        from: source_coord,
                        to: dest_coord,
                        promote_to: if is_promotion { Some(promote_to) } else { None },
                    }))
                }
            },
            PieceDragSource::Reserve => {
                Ok(Turn::Drop(TurnDrop {
                    piece_kind,
                    to: dest_coord
                }))
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
    }
}
