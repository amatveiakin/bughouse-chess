use crate::board::{Turn, TurnError};
use crate::clock::GameInstant;
use crate::coord::{SubjectiveRow, Coord};
use crate::force::Force;
use crate::game::{BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::piece::{PieceKind, piece_to_full_algebraic, piece_to_algebraic_for_move};


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
    my_name: String,
    my_board: BughouseBoard,
    my_force: Force,
    // State as it has been confirmed by the server.
    game_confirmed: BughouseGame,
    // Local turn, unconfirmed by the server yet, but displayed on the client.
    // This is always a valid turn for the `game_confirmed`.
    local_turn: Option<(Turn, GameInstant)>,
    // Drag&drop state if making turn by mouse or touch.
    piece_drag: Option<PieceDrag>,
}

impl AlteredGame {
    pub fn new(my_name: String, game_confirmed: BughouseGame) -> Self {
        let (my_board, my_force) = game_confirmed.find_player(&my_name).unwrap();
        AlteredGame {
            my_name,
            my_board,
            my_force,
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
        self.game_confirmed.set_status(status, time)
    }

    pub fn apply_remote_turn_algebraic(
        &mut self, player_name: &str, turn_algebraic: &str, time: GameInstant)
        -> Result<Turn, TurnError>
    {
        if player_name == self.my_name {
            self.local_turn = None;
        }
        let turn = self.game_confirmed.try_turn_algebraic_by_player(
            &player_name, &turn_algebraic, time
        )?;
        if let Some(ref mut drag) = self.piece_drag {
            if let PieceDragSource::Board(coord) = drag.source {
                let board = self.game_confirmed.board(self.my_board);
                if board.grid()[coord].map(|piece| piece.force) != Some(self.my_force) {
                    // Dragged piece was captured by opponent.
                    drag.source = PieceDragSource::Defunct;
                }
            }
        }
        Ok(turn)
    }

    pub fn my_name(&self) -> &str { &self.my_name }
    pub fn my_board(&self) -> BughouseBoard { self.my_board }
    pub fn my_force(&self) -> Force { self.my_force }

    // Improvement potential: Move everything related to players and sitting out of game
    //   classes and give direct access to it.
    pub fn find_player(&self, player_name: &str) -> Option<(BughouseBoard, Force)> {
        self.game_confirmed.find_player(player_name)
    }
    pub fn are_opponents(&self, player_name_a: &str, player_name_b: &str) -> Option<bool> {
        self.game_confirmed.are_opponents(player_name_a, player_name_b)
    }

    pub fn local_game(&self) -> BughouseGame {
        let mut game = self.game_confirmed.clone();
        if let Some((turn, turn_time)) = self.local_turn {
            // Note. Not calling `test_flag`, because only server records flag defeat.
            // TODO: Debug: This has paniced in production.
            game.try_turn_by_player(&self.my_name, turn, turn_time).unwrap();
        }
        if let Some(ref drag) = self.piece_drag {
            let board = game.board_mut(self.my_board);
            match drag.source {
                PieceDragSource::Defunct => {},
                PieceDragSource::Board(coord) => {
                    let piece = board.grid_mut()[coord].take().unwrap();
                    assert_eq!(piece.force, self.my_force);
                    assert_eq!(piece.kind, drag.piece_kind);
                },
                PieceDragSource::Reserve => {
                    let reserve = board.reserve_mut(self.my_force);
                    assert!(reserve[drag.piece_kind] > 0);
                    reserve[drag.piece_kind] -= 1;
                }
            }
        }
        game
    }

    pub fn can_make_local_turn(&self) -> bool {
        self.game_confirmed.player_is_active(&self.my_name).unwrap() && self.local_turn.is_none()
    }

    pub fn try_local_turn_algebraic(&mut self, turn_algebraic: &str, time: GameInstant)
        -> Result<(), TurnError>
    {
        let mut game_copy = self.game_confirmed.clone();
        let turn = game_copy.try_turn_algebraic_by_player(
            &self.my_name, turn_algebraic, time
        )?;
        self.local_turn = Some((turn, time));
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
                let board = game.player_board(&self.my_name).unwrap();
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

    // Stop drag and returns algebraic turn notation on success.
    // Improvement potential: Return `Turn` struct instead of turn algebraic.
    pub fn drag_piece_drop(&mut self, dest_coord: Coord, promote_to: PieceKind)
        -> Result<String, PieceDragError>
    {
        let drag = self.piece_drag.as_ref().ok_or(PieceDragError::NoDragInProgress)?;
        let piece_kind = drag.piece_kind;
        let source = drag.source;
        self.piece_drag = None;

        let dest_notation = dest_coord.to_algebraic();
        match source {
            PieceDragSource::Defunct => {
                Err(PieceDragError::DragNoLongerPossible)
            },
            PieceDragSource::Board(source_coord) => {
                use PieceKind::*;
                let game = self.local_game();
                let board = game.player_board(&self.my_name).unwrap();
                let first_row = SubjectiveRow::from_one_based(1).to_row(self.my_force);
                let last_row = SubjectiveRow::from_one_based(8).to_row(self.my_force);
                let d_col = dest_coord.col - source_coord.col;
                let d_col_abs = d_col.abs();
                let source_notation = source_coord.to_algebraic();
                let to_my_piece = if let Some(piece_to) = board.grid()[dest_coord] {
                    piece_to.force == self.my_force
                } else {
                    false
                };
                // Castling rules: drag the king at least two squares in the rook direction
                // or onto a friendly piece. That later is required for Fischer random where
                // a king could start on b1 or g1.
                let is_castling =
                    piece_kind == King &&
                    (d_col_abs >= 2 || (d_col_abs >= 1 && to_my_piece)) &&
                    (source_coord.row == first_row && dest_coord.row == first_row)
                ;
                let is_promotion = piece_kind == Pawn && dest_coord.row == last_row;
                if is_castling {
                    Ok((if d_col > 0 { "0-0" } else { "0-0-0" }).to_owned())
                } else if is_promotion {
                    let promotion_str = piece_to_full_algebraic(promote_to);
                    Ok(format!("{}{}/{}", source_notation, dest_notation, promotion_str))
                } else {
                    let piece_str = piece_to_algebraic_for_move(piece_kind);
                    Ok(format!("{}{}{}", piece_str, source_notation, dest_notation))
                }
            },
            PieceDragSource::Reserve => {
                let piece_str = piece_to_full_algebraic(piece_kind);
                Ok(format!("{}@{}", piece_str, dest_notation))
            }
        }
    }

    pub fn reset_local_changes(&mut self) {
        self.local_turn = None;
    }
}
