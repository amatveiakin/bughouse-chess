// Defines `AlteredGame` and auxiliary classes. `AlteredGame` is used for online multiplayer
// and represents a game with local changes not (yet) confirmed by the server.
//
// The general philosophy is that server is trusted, but the user is not. `AlteredGame` may
// (or may not) panic if server command doesn't make sense (e.g. invalid chess move), but it
// shall not panic on bogus local turns and other invalid user actions.
//
// Only one preturn is allowed, but it's possible to have two unconfirmed local turns: one
// normal and one preturn.

use std::cmp;
use std::collections::HashSet;
use std::rc::Rc;

use enum_map::{enum_map, EnumMap};
use strum::IntoEnumIterator;

use crate::board::{Turn, TurnDrop, TurnError, TurnInput, TurnMode, TurnMove};
use crate::clock::GameInstant;
use crate::coord::{Coord, SubjectiveRow};
use crate::display::Perspective;
use crate::game::{
    get_bughouse_force, BughouseEnvoy, BughouseGame, BughouseGameStatus, BughouseParticipant,
};
use crate::piece::{CastleDirection, PieceKind};
use crate::rules::{BughouseRules, ChessRules, ChessVariant};
use crate::BughouseBoard;


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
    Defunct, // dragged piece captured by opponent or depends on a cancelled preturn
    Board(Coord),
    Reserve,
}

#[derive(Clone, Debug)]
pub struct PieceDrag {
    pub board_idx: BughouseBoard,
    pub piece_kind: PieceKind,
    pub source: PieceDragSource,
}

#[derive(Default, Clone, Debug)]
struct Alterations {
    // Local turn (TurnMode::Normal) not confirmed by the server yet, but displayed on the
    // client. Always a valid turn for the `game_confirmed`.
    local_turn: Option<(TurnInput, GameInstant)>,
    // Local preturn (TurnMode::Preturn). Executed after `local_turn` if the latter exists.
    local_preturn: Option<(TurnInput, GameInstant)>,
}

#[derive(Clone, Debug)]
pub struct AlteredGame {
    // All local actions are assumed to be made on behalf of this player.
    my_id: BughouseParticipant,
    // State as it has been confirmed by the server.
    game_confirmed: BughouseGame,
    // Local changes to the game state.
    alterations: EnumMap<BughouseBoard, Alterations>,
    // Drag&drop state if making turn by mouse or touch.
    piece_drag: Option<PieceDrag>,
}

impl AlteredGame {
    pub fn new(my_id: BughouseParticipant, game_confirmed: BughouseGame) -> Self {
        AlteredGame {
            my_id,
            game_confirmed,
            alterations: enum_map! { _ => Alterations::default() },
            piece_drag: None,
        }
    }

    pub fn chess_rules(&self) -> &Rc<ChessRules> { self.game_confirmed().chess_rules() }
    pub fn bughouse_rules(&self) -> &Rc<BughouseRules> { self.game_confirmed().bughouse_rules() }

    // Status returned by this function may differ from `local_game()` status.
    // This function should be used as the source of truth when showing game status to the
    // user, as it's possible that the final status from the server will be different, e.g.
    // if the game ended earlier on the other board.
    pub fn status(&self) -> BughouseGameStatus { self.game_confirmed.status() }
    pub fn is_active(&self) -> bool { self.game_confirmed.is_active() }

    pub fn set_status(&mut self, status: BughouseGameStatus, time: GameInstant) {
        assert!(!status.is_active());
        self.reset_local_changes();
        self.game_confirmed.set_status(status, time);
    }

    pub fn apply_remote_turn_algebraic(
        &mut self, envoy: BughouseEnvoy, turn_algebraic: &str, time: GameInstant,
    ) -> Result<Turn, TurnError> {
        let turn_input = TurnInput::Algebraic(turn_algebraic.to_owned());
        let turn =
            self.game_confirmed
                .try_turn_by_envoy(envoy, &turn_input, TurnMode::Normal, time)?;

        if !self.game_confirmed.is_active() {
            self.reset_local_changes();
            return Ok(turn);
        }

        if self.my_id.plays_on_board(envoy.board_idx) {
            let alterations = &mut self.alterations[envoy.board_idx];
            // Something on our board has changed. Time to figure out which local turns to keep.
            // An alternative solution would've been to annotate each turn with turn number and
            // remove all obsolete turns. But that's boring. So here we go...
            if self.my_id.plays_for(envoy) {
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
                if alterations.local_turn.is_none() {
                    assert!(alterations.local_preturn.is_none());
                }
                alterations.local_turn = None;
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
                assert!(alterations.local_turn.is_none());
                if let Some((turn_input, original_time)) = alterations.local_preturn.take() {
                    // Make sure we don't go back in time by making a turn before a confirmed
                    // opponent's turn. An alternative solution would be to allow only `Approximate`
                    // time measurement everywhere in the client code (including tests).
                    let my_board = self.game_confirmed.board(envoy.board_idx);
                    let opponent_turn_time = my_board.clock().total_time_elapsed();
                    let t = cmp::max(original_time.elapsed_since_start(), opponent_turn_time);
                    let turn_time =
                        GameInstant::from_duration(t).set_measurement(original_time.measurement());
                    // A-a-and we are ready to reapply the preturn. As a normal turn now.
                    // Ignore any errors: it's normal for preturns to fail.
                    _ = self.try_local_turn_ignore_drag(envoy.board_idx, turn_input, turn_time);
                }
            }
        }

        let mut game = self.game_with_local_turns(true);
        if self.apply_drag(&mut game).is_err() {
            let Some(ref mut drag) = self.piece_drag else {
                panic!("Got a drag failure with no drag in progress");
            };
            // Drag invalidated. Possible reasons: dragged piece was captured by opponent;
            // dragged piece depended on a preturn that was cancelled.
            drag.source = PieceDragSource::Defunct;
        }
        Ok(turn)
    }

    pub fn my_id(&self) -> BughouseParticipant { self.my_id }
    pub fn perspective(&self) -> Perspective { Perspective::for_participant(self.my_id) }
    pub fn game_confirmed(&self) -> &BughouseGame { &self.game_confirmed }

    pub fn local_game(&self) -> BughouseGame {
        let mut game = self.game_with_local_turns(true);
        self.apply_drag(&mut game).unwrap();
        game
    }

    pub fn fog_of_war_area(&self, board_idx: BughouseBoard) -> HashSet<Coord> {
        match self.chess_rules().chess_variant {
            ChessVariant::Standard => HashSet::new(),
            ChessVariant::FogOfWar => {
                if let BughouseParticipant::Player(my_player_id) = self.my_id {
                    // Don't use `local_game`: preturns and drags should not reveal new areas.
                    let mut game = self.game_with_local_turns(false);
                    let force = get_bughouse_force(my_player_id.team(), board_idx);
                    let mut visible = game.board(board_idx).fog_free_area(force);
                    // ... but do show preturn pieces themselves:
                    if let Some((ref turn_input, turn_time)) =
                        self.alterations[board_idx].local_preturn
                    {
                        let envoy = self.my_id.envoy_for(board_idx).unwrap();
                        game.try_turn_by_envoy(envoy, turn_input, TurnMode::Preturn, turn_time)
                            .unwrap();
                        let turn_expanded = &game.last_turn_record().unwrap().turn_expanded;
                        if let Some((_, sq)) = turn_expanded.relocation {
                            visible.insert(sq);
                        }
                        if let Some((_, sq)) = turn_expanded.relocation_extra {
                            visible.insert(sq);
                        }
                        if let Some(sq) = turn_expanded.drop {
                            visible.insert(sq);
                        }
                    }
                    Coord::all().filter(|c| !visible.contains(c)).collect()
                } else {
                    HashSet::new()
                }
            }
        }
    }

    pub fn try_local_turn(
        &mut self, board_idx: BughouseBoard, turn_input: TurnInput, time: GameInstant,
    ) -> Result<TurnMode, TurnError> {
        let mode = self.try_local_turn_ignore_drag(board_idx, turn_input, time)?;
        self.piece_drag = None;
        Ok(mode)
    }

    pub fn piece_drag_state(&self) -> &Option<PieceDrag> { &self.piece_drag }

    pub fn start_drag_piece(
        &mut self, board_idx: BughouseBoard, start: PieceDragStart,
    ) -> Result<(), PieceDragError> {
        let BughouseParticipant::Player(my_player_id) = self.my_id else {
            return Err(PieceDragError::DragForbidden);
        };
        let Some(my_envoy) = my_player_id.envoy_for(board_idx) else {
            return Err(PieceDragError::DragForbidden);
        };
        if self.piece_drag.is_some() {
            return Err(PieceDragError::DragAlreadyStarted);
        }
        let game = self.game_with_local_turns(true);
        let board = game.board(board_idx);
        let (piece_kind, source) = match start {
            PieceDragStart::Board(coord) => {
                let piece = board.grid()[coord].ok_or(PieceDragError::PieceNotFound)?;
                if piece.force != my_envoy.force {
                    return Err(PieceDragError::DragForbidden);
                }
                (piece.kind, PieceDragSource::Board(coord))
            }
            PieceDragStart::Reserve(piece_kind) => {
                if board.reserve(my_envoy.force)[piece_kind] <= 0 {
                    return Err(PieceDragError::PieceNotFound);
                }
                (piece_kind, PieceDragSource::Reserve)
            }
        };
        self.piece_drag = Some(PieceDrag { board_idx, piece_kind, source });
        Ok(())
    }

    pub fn abort_drag_piece(&mut self) { self.piece_drag = None; }

    // Stop drag and returns turn on success. The client should then manually apply this
    // turn via `make_turn`.
    pub fn drag_piece_drop(
        &mut self, dest: Coord, promote_to: PieceKind,
    ) -> Result<TurnInput, PieceDragError> {
        let BughouseParticipant::Player(my_player_id) = self.my_id else {
            return Err(PieceDragError::DragForbidden);
        };
        let PieceDrag { board_idx, piece_kind, source } =
            self.piece_drag.take().ok_or(PieceDragError::NoDragInProgress)?;

        match source {
            PieceDragSource::Defunct => Err(PieceDragError::DragNoLongerPossible),
            PieceDragSource::Board(source_coord) => {
                use PieceKind::*;
                if source_coord == dest {
                    return Err(PieceDragError::Cancelled);
                }
                // Unwrap ok: cannot start the drag if not playing on this board.
                let force = my_player_id.envoy_for(board_idx).unwrap().force;
                let first_row = SubjectiveRow::from_one_based(1).unwrap().to_row(force);
                let last_row = SubjectiveRow::from_one_based(8).unwrap().to_row(force);
                let d_col = dest.col - source_coord.col;
                let is_castling = piece_kind == King
                    && (d_col.abs() >= 2)
                    && (source_coord.row == first_row && dest.row == first_row);
                let is_promotion = piece_kind == Pawn && dest.row == last_row;
                if is_castling {
                    use CastleDirection::*;
                    let dir = if d_col > 0 { HSide } else { ASide };
                    Ok(TurnInput::DragDrop(Turn::Castle(dir)))
                } else {
                    Ok(TurnInput::DragDrop(Turn::Move(TurnMove {
                        from: source_coord,
                        to: dest,
                        promote_to: if is_promotion { Some(promote_to) } else { None },
                    })))
                }
            }
            PieceDragSource::Reserve => {
                Ok(TurnInput::DragDrop(Turn::Drop(TurnDrop { piece_kind, to: dest })))
            }
        }
    }

    pub fn cancel_preturn(&mut self, board_idx: BughouseBoard) -> bool {
        // Note: Abort drag just to be safe. In practice existing GUI doesn't allow to
        // cancel preturn while dragging. If this is desired, a proper check needs to be
        // done (like in `apply_remote_turn_algebraic`).
        self.piece_drag = None;
        self.alterations[board_idx].local_preturn.take().is_some()
    }

    fn reset_local_changes(&mut self) {
        self.alterations = enum_map! { _ => Alterations::default() };
        self.piece_drag = None;
    }

    fn game_with_local_turns(&self, include_preturns: bool) -> BughouseGame {
        let mut game = self.game_confirmed.clone();
        for board_idx in BughouseBoard::iter() {
            let Some(envoy) = self.my_id.envoy_for(board_idx) else {
                continue;
            };
            let alterations = &self.alterations[board_idx];
            // Note. Not calling `test_flag`, because only server records flag defeat.
            // Unwrap ok: turn correctness (according to the `mode`) has already been verified.
            if let Some((ref turn_input, turn_time)) = alterations.local_turn {
                game.try_turn_by_envoy(envoy, turn_input, TurnMode::Normal, turn_time).unwrap();
            }
            if include_preturns {
                if let Some((ref turn_input, turn_time)) = alterations.local_preturn {
                    game.try_turn_by_envoy(envoy, turn_input, TurnMode::Preturn, turn_time)
                        .unwrap();
                }
            }
        }
        game
    }

    fn apply_drag(&self, game: &mut BughouseGame) -> Result<(), String> {
        let Some(ref drag) = self.piece_drag else {
            return Ok(());
        };
        // Unwrap ok: cannot start the drag if not playing on this board.
        let envoy = self.my_id.envoy_for(drag.board_idx).unwrap();
        let board = game.board_mut(drag.board_idx);
        match drag.source {
            PieceDragSource::Defunct => {}
            PieceDragSource::Board(coord) => {
                let piece = board.grid_mut()[coord].take().unwrap(); // note: `take` modifies the board
                let expected = (envoy.force, drag.piece_kind);
                let actual = (piece.force, piece.kind);
                if expected != actual {
                    return Err(format!(
                        "Drag piece mismatch. Expected {expected:?}, found {actual:?}"
                    ));
                }
            }
            PieceDragSource::Reserve => {
                let reserve = board.reserve_mut(envoy.force);
                if reserve[drag.piece_kind] <= 0 {
                    return Err(format!(
                        "Drag piece missing in reserve: {:?} {:?}",
                        envoy.force, drag.piece_kind
                    ));
                }
                reserve[drag.piece_kind] -= 1;
            }
        }
        Ok(())
    }

    fn try_local_turn_ignore_drag(
        &mut self, board_idx: BughouseBoard, turn_input: TurnInput, time: GameInstant,
    ) -> Result<TurnMode, TurnError> {
        let Some(envoy) = self.my_id.envoy_for(board_idx) else {
            return Err(TurnError::NotPlayer);
        };
        if self.alterations[board_idx].local_preturn.is_some() {
            return Err(TurnError::PreturnLimitReached);
        }
        let mut game = self.game_with_local_turns(true);
        let mode = game.turn_mode_for_envoy(envoy)?;
        game.try_turn_by_envoy(envoy, &turn_input, mode, time)?;
        let alterations = &mut self.alterations[board_idx];
        match mode {
            TurnMode::Normal => alterations.local_turn = Some((turn_input, time)),
            TurnMode::Preturn => alterations.local_preturn = Some((turn_input, time)),
        };
        Ok(mode)
    }
}
