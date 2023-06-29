// Defines `AlteredGame` and auxiliary classes. `AlteredGame` is used for online multiplayer
// and represents a game with local changes not (yet) confirmed by the server.
//
// The general philosophy is that server is trusted, but the user is not. `AlteredGame` may
// (or may not) panic if server command doesn't make sense (e.g. invalid chess move), but it
// shall not panic on bogus local turns and other invalid user actions.
//
// Only one preturn is allowed (for game-design reasons, this is not a technical limitation).
// It is still possible to have two unconfirmed local turns: one normal and one preturn.

// Improvement potential: Reduce the number of times large entities are recomputed
// (e.g.`turn_highlights` recomputes `local_game` and `fog_of_war_area`, which are presumably
// already available by the time it's used). Ideas:
//   - Cache latest result and reevaluate when invalidated;
//   - Replace all existing read-only methods with one "get visual representation" method that
//     contain all the information required in order to render the game.

use std::collections::HashSet;
use std::rc::Rc;

use enum_map::{enum_map, EnumMap};
use itertools::Itertools;
use strum::IntoEnumIterator;

use crate::board::{
    PromotionTarget, Turn, TurnDrop, TurnError, TurnExpanded, TurnInput, TurnMode, TurnMove,
};
use crate::clock::GameInstant;
use crate::coord::{BoardShape, Coord, SubjectiveRow};
use crate::display::Perspective;
use crate::game::{
    get_bughouse_force, BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus,
    BughouseParticipant, TurnRecord, TurnRecordExpanded,
};
use crate::piece::{CastleDirection, PieceForce, PieceKind};
use crate::rules::{BughouseRules, ChessRules, ChessVariant, Promotion};


#[derive(Clone, Copy, Debug)]
pub enum PartialTurnInput {
    Drag {
        piece_kind: PieceKind,
        piece_force: PieceForce,
        source: PieceDragSource,
    },
    StealPromotion {
        from: Coord,
        to: Coord,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum PieceDragStart {
    Board(Coord),
    Reserve(PieceKind),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceDragError {
    DragForbidden,
    DragIllegal,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceDragState {
    NoDrag,
    Dragging,
    Defunct,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnHighlightLayer {
    BelowFog,
    AboveFog,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnHighlightFamily {
    PartialTurn,
    Preturn,
    LatestTurn,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnHighlightItem {
    MoveFrom,
    MoveTo,
    Drop,
    Capture,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TurnHighlight {
    pub board_idx: BughouseBoard,
    pub coord: Coord,
    pub layer: TurnHighlightLayer,
    pub family: TurnHighlightFamily,
    pub item: TurnHighlightItem,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LocalTurns {
    OnlyNormal,   // normal
    NormalAndPre, // normal + pre
    All,          // normal + pre + partial
}

#[derive(Clone, Debug)]
pub struct AlteredGame {
    // All local actions are assumed to be made on behalf of this player.
    my_id: BughouseParticipant,
    // State as it has been confirmed by the server.
    game_confirmed: BughouseGame,
    // Partial turn input (e.g. move of a pawn to the last rank without a promotion choice).
    partial_turn_input: Option<(BughouseBoard, PartialTurnInput)>,
    // Local changes of two kinds:
    //   - Local turns (TurnMode::Normal) not confirmed by the server yet, but displayed on the
    //     client. Always valid turns for the `game_confirmed`.
    //   - Local preturns (TurnMode::Preturn).
    // The turns are executed sequentially. Preturns always follow normal turns.
    local_turns: Vec<TurnRecord>,
    // Historical position that the user is currently viewing.
    wayback_turn_index: EnumMap<BughouseBoard, Option<String>>,
}

impl AlteredGame {
    pub fn new(my_id: BughouseParticipant, game_confirmed: BughouseGame) -> Self {
        AlteredGame {
            my_id,
            game_confirmed,
            partial_turn_input: None,
            local_turns: Vec::new(),
            wayback_turn_index: enum_map! { _ => None },
        }
    }

    pub fn chess_rules(&self) -> &Rc<ChessRules> { self.game_confirmed().chess_rules() }
    pub fn bughouse_rules(&self) -> &Rc<BughouseRules> { self.game_confirmed().bughouse_rules() }
    pub fn board_shape(&self) -> BoardShape { self.game_confirmed().board_shape() }

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

    pub fn apply_remote_turn(
        &mut self, envoy: BughouseEnvoy, turn_input: &TurnInput, time: GameInstant,
    ) -> Result<TurnRecordExpanded, TurnError> {
        let mut original_game_confirmed = self.game_confirmed.clone();
        self.game_confirmed
            .try_turn_by_envoy(envoy, turn_input, TurnMode::Normal, time)?;
        let remote_turn_record = self.game_confirmed.turn_log().last().unwrap().clone();

        if !self.game_confirmed.is_active() {
            self.reset_local_changes();
            return Ok(remote_turn_record);
        }

        for (turn_idx, turn_record) in self.local_turns.iter().enumerate() {
            if turn_record.envoy == envoy {
                let local_turn =
                    original_game_confirmed.apply_turn_record(turn_record, TurnMode::Normal);
                if local_turn == Ok(remote_turn_record.turn_expanded.turn) {
                    // The server confirmed a turn made by this player. Discard the local copy.
                    self.local_turns.remove(turn_idx);
                    break;
                } else {
                    // The server sent a turn made by this player, but it's different from the local
                    // turn. The entire turn sequence on this board is now invalid. One way this
                    // could have happened is if the user made a preturn, cancelled it and made new
                    // preturns locally, but the cancellation didn't get to the server in time, so
                    // the server applied earlier preturn version. We don't want to apply subsequent
                    // preturns based on a different game history: if the first turn changed, the
                    // user probably wants to make different turns based on that.
                    self.local_turns.retain(|r| r.envoy != envoy);
                    break;
                }
            }
        }

        self.discard_invalid_local_turns();
        Ok(remote_turn_record)
    }

    pub fn my_id(&self) -> BughouseParticipant { self.my_id }
    pub fn perspective(&self) -> Perspective { Perspective::for_participant(self.my_id) }
    pub fn game_confirmed(&self) -> &BughouseGame { &self.game_confirmed }

    pub fn local_game(&self) -> BughouseGame {
        let mut game = self.game_with_local_turns(LocalTurns::All);
        self.apply_wayback(&mut game);
        game
    }

    pub fn partial_turn_input(&self) -> Option<(BughouseBoard, PartialTurnInput)> {
        self.partial_turn_input
    }

    pub fn is_my_duck_turn(&self, board_idx: BughouseBoard) -> bool {
        let Some(envoy) = self.my_id.envoy_for(board_idx) else {
            return false;
        };
        let game = self.game_with_local_turns(LocalTurns::All);
        game.board(board_idx).is_duck_turn(envoy.force)
    }

    pub fn turn_highlights(&self) -> Vec<TurnHighlight> {
        let my_id = self.my_id;
        let game = self.local_game();
        let mut highlights = vec![];

        for board_idx in BughouseBoard::iter() {
            let see_though_fog = self.see_though_fog();
            let empty_area = HashSet::new();
            let fog_render_area = self.fog_of_war_area(board_idx);
            let fog_cover_area = if see_though_fog { &empty_area } else { &fog_render_area };

            let wayback_turn_idx = self.wayback_turn_index(board_idx);
            let turn_log = board_turn_log_modulo_wayback(&game, board_idx, wayback_turn_idx);

            if let Some(latest_turn_record) = turn_log.last() {
                if !my_id.plays_for(latest_turn_record.envoy) || wayback_turn_idx.is_some() {
                    // Highlight all components of the latest megaturn. Normally this would be exactly one
                    // turn, but in duck chess it's both the regular piece and the duck.
                    for r in
                        turn_log.iter().rev().take_while(|r| r.envoy == latest_turn_record.envoy)
                    {
                        highlights.extend(get_turn_highlights(
                            TurnHighlightFamily::LatestTurn,
                            board_idx,
                            &r.turn_expanded,
                            fog_cover_area,
                        ));
                    }
                }
            }

            for r in turn_log.iter() {
                if r.mode == TurnMode::Preturn {
                    highlights.extend(get_turn_highlights(
                        TurnHighlightFamily::Preturn,
                        board_idx,
                        &r.turn_expanded,
                        fog_cover_area,
                    ));
                }
            }

            if let Some((input_board_idx, ref partial_input)) = self.partial_turn_input {
                if input_board_idx == board_idx {
                    highlights.extend(get_partial_turn_highlights(
                        board_idx,
                        partial_input,
                        fog_cover_area,
                    ));
                }
            }
        }
        highlights
            .into_iter()
            .into_grouping_map_by(|h| (h.board_idx, h.coord))
            .max_by_key(|_, h| turn_highlight_z_index(h))
            .into_values()
            .collect()
    }

    pub fn see_though_fog(&self) -> bool { !self.is_active() }

    pub fn fog_of_war_area(&self, board_idx: BughouseBoard) -> HashSet<Coord> {
        match self.chess_rules().chess_variant {
            ChessVariant::Standard => HashSet::new(),
            ChessVariant::FogOfWar => {
                if let BughouseParticipant::Player(my_player_id) = self.my_id {
                    // Don't use `local_game`: preturns and drags should not reveal new areas.
                    let mut game = self.game_with_local_turns(LocalTurns::OnlyNormal);
                    let board_shape = self.board_shape();
                    let wayback_active = self.apply_wayback_for_board(&mut game, board_idx);
                    let force = get_bughouse_force(my_player_id.team(), board_idx);
                    let mut visible = game.board(board_idx).fog_free_area(force);
                    // ... but do show preturn pieces themselves:
                    if !wayback_active {
                        let game_with_preturns = self.game_with_local_turns(LocalTurns::All);
                        for coord in board_shape.coords() {
                            if let Some(piece) = game_with_preturns.board(board_idx).grid()[coord] {
                                if piece.force.is_owned_by_or_neutral(force) {
                                    visible.insert(coord);
                                }
                            }
                        }
                    }
                    board_shape.coords().filter(|c| !visible.contains(c)).collect()
                } else {
                    HashSet::new()
                }
            }
        }
    }

    pub fn try_local_turn(
        &mut self, board_idx: BughouseBoard, turn_input: TurnInput, time: GameInstant,
    ) -> Result<TurnMode, TurnError> {
        let Some(envoy) = self.my_id.envoy_for(board_idx) else {
            return Err(TurnError::NotPlayer);
        };
        if self.wayback_turn_index[board_idx].is_some() {
            return Err(TurnError::WaybackIsActive);
        }
        if self.num_preturns_on_board(board_idx) >= self.chess_rules().max_preturns_per_board() {
            return Err(TurnError::PreturnLimitReached);
        }
        self.partial_turn_input = None;
        let mut game = self.game_with_local_turns(LocalTurns::All);
        let mode = game.turn_mode_for_envoy(envoy)?;
        game.try_turn_by_envoy(envoy, &turn_input, mode, time)?;
        // Note: cannot use `game.turn_log().last()` here! It will change the input method, and this
        // can cause subtle differences in preturn execution. For example, when making algebraic
        // turns you can require the turn be capturing by using "x". This information will be lost
        // if using TurnInput::Explicit.
        self.local_turns.push(TurnRecord { envoy, turn_input, time });
        Ok(mode)
    }

    pub fn wayback_turn_index(&self, board_idx: BughouseBoard) -> Option<&str> {
        self.wayback_turn_index[board_idx].as_deref()
    }
    pub fn wayback_to_turn(&mut self, board_idx: BughouseBoard, turn_idx: Option<String>) {
        self.cancel_partial_turn_on_board(board_idx);
        let last_turn_idx = self
            .local_game()
            .turn_log()
            .iter()
            .rev()
            .find(|record| record.envoy.board_idx == board_idx)
            .map(|record| record.index());
        let turn_idx = if turn_idx == last_turn_idx { None } else { turn_idx };
        self.wayback_turn_index[board_idx] = turn_idx;
    }

    // Improvement: Less ad-hoc solution for "gluing" board index to TurnInput; use it here, in
    // `drag_piece_drop` and in `BughouseClientEvent::MakeTurn`.
    pub fn click_square(
        &mut self, board_idx: BughouseBoard, coord: Coord,
    ) -> Option<(BughouseBoard, TurnInput)> {
        if let Some((input_board_idx, partial_input)) = self.partial_turn_input {
            match partial_input {
                PartialTurnInput::Drag { .. } => {
                    self.partial_turn_input = None;
                }
                PartialTurnInput::StealPromotion { from, to } => {
                    if board_idx == input_board_idx.other() {
                        let game = self.game_with_local_turns(LocalTurns::All);
                        if let Some(piece) = game.board(board_idx).grid()[coord] {
                            let full_input = TurnInput::DragDrop(Turn::Move(TurnMove {
                                from,
                                to,
                                promote_to: Some(PromotionTarget::Steal((piece.kind, piece.id))),
                            }));
                            self.partial_turn_input = None;
                            return Some((input_board_idx, full_input));
                        }
                    }
                }
            }
        } else if self.is_my_duck_turn(board_idx) {
            // Improvement potential: Also allow to make regular moves in two clicks.
            return Some((board_idx, TurnInput::DragDrop(Turn::PlaceDuck(coord))));
        }
        None
    }

    pub fn piece_drag_state(&self) -> PieceDragState {
        match self.partial_turn_input {
            Some((_, PartialTurnInput::Drag { source, .. })) => match source {
                PieceDragSource::Defunct => PieceDragState::Defunct,
                _ => PieceDragState::Dragging,
            },
            _ => PieceDragState::NoDrag,
        }
    }

    pub fn start_drag_piece(
        &mut self, board_idx: BughouseBoard, start: PieceDragStart,
    ) -> Result<(), PieceDragError> {
        let BughouseParticipant::Player(my_player_id) = self.my_id else {
            return Err(PieceDragError::DragForbidden);
        };
        let Some(my_envoy) = my_player_id.envoy_for(board_idx) else {
            return Err(PieceDragError::DragForbidden);
        };
        if self.wayback_turn_index[board_idx].is_some() {
            return Err(PieceDragError::DragForbidden);
        }
        if let Some((input_board_idx, _)) = self.partial_turn_input {
            if input_board_idx == board_idx {
                return Err(PieceDragError::DragForbidden);
            }
        }
        self.partial_turn_input = None;
        let game = self.game_with_local_turns(LocalTurns::All);
        let board = game.board(board_idx);
        let (piece_kind, piece_force, source) = match start {
            PieceDragStart::Board(coord) => {
                let piece = board.grid()[coord].ok_or(PieceDragError::PieceNotFound)?;
                if !board.can_potentially_move_piece(my_envoy.force, piece.force) {
                    return Err(PieceDragError::DragForbidden);
                }
                (piece.kind, piece.force, PieceDragSource::Board(coord))
            }
            PieceDragStart::Reserve(piece_kind) => {
                if board.reserve(my_envoy.force)[piece_kind] == 0 {
                    return Err(PieceDragError::PieceNotFound);
                }
                let piece_force = piece_kind.reserve_piece_force(my_envoy.force);
                (piece_kind, piece_force, PieceDragSource::Reserve)
            }
        };
        self.try_partial_turn(board_idx, PartialTurnInput::Drag {
            piece_kind,
            piece_force,
            source,
        })
        .map_err(|()| PieceDragError::DragIllegal)?;
        Ok(())
    }

    pub fn abort_drag_piece(&mut self) {
        if matches!(self.partial_turn_input, Some((_, PartialTurnInput::Drag { .. }))) {
            self.partial_turn_input = None;
        }
    }

    // Stop drag and returns turn on success. The client should then manually apply this
    // turn via `make_turn`.
    pub fn drag_piece_drop(
        &mut self, board_idx: BughouseBoard, dest: Coord, promote_to: PieceKind,
    ) -> Result<Option<TurnInput>, PieceDragError> {
        let Some((input_board_idx, PartialTurnInput::Drag { piece_kind, piece_force, source })) = self.partial_turn_input else {
            return Err(PieceDragError::NoDragInProgress);
        };
        self.partial_turn_input = None;
        if input_board_idx != board_idx {
            return Err(PieceDragError::DragForbidden);
        }

        match source {
            PieceDragSource::Defunct => Err(PieceDragError::DragNoLongerPossible),
            PieceDragSource::Board(source_coord) => {
                use PieceKind::*;
                if source_coord == dest {
                    return Err(PieceDragError::Cancelled);
                }
                if piece_kind == PieceKind::Duck {
                    return Ok(Some(TurnInput::DragDrop(Turn::PlaceDuck(dest))));
                }
                let board_shape = self.board_shape();
                let d_col = dest.col - source_coord.col;
                let mut is_castling = false;
                let mut is_promotion = false;
                if let Ok(force) = piece_force.try_into() {
                    let first_row = SubjectiveRow::first().to_row(board_shape, force);
                    let last_row = SubjectiveRow::last(board_shape).to_row(board_shape, force);
                    is_castling = piece_kind == King
                        && (d_col.abs() >= 2)
                        && (source_coord.row == first_row && dest.row == first_row);
                    is_promotion = piece_kind == Pawn && dest.row == last_row;
                }
                if is_castling {
                    use CastleDirection::*;
                    let dir = if d_col > 0 { HSide } else { ASide };
                    Ok(Some(TurnInput::DragDrop(Turn::Castle(dir))))
                } else {
                    if is_promotion {
                        match self.bughouse_rules().promotion {
                            // TODO: Use partial_turn_input to allow proper promotion target choice.
                            Promotion::Upgrade => {
                                Ok(Some(TurnInput::DragDrop(Turn::Move(TurnMove {
                                    from: source_coord,
                                    to: dest,
                                    promote_to: Some(PromotionTarget::Upgrade(promote_to)),
                                }))))
                            }
                            Promotion::Discard => {
                                Ok(Some(TurnInput::DragDrop(Turn::Move(TurnMove {
                                    from: source_coord,
                                    to: dest,
                                    promote_to: Some(PromotionTarget::Discard),
                                }))))
                            }
                            Promotion::Steal => {
                                self.try_partial_turn(
                                    board_idx,
                                    PartialTurnInput::StealPromotion {
                                        from: source_coord,
                                        to: dest,
                                    },
                                )
                                .map_err(|()| PieceDragError::DragIllegal)?;
                                Ok(None)
                            }
                        }
                    } else {
                        Ok(Some(TurnInput::DragDrop(Turn::Move(TurnMove {
                            from: source_coord,
                            to: dest,
                            promote_to: None,
                        }))))
                    }
                }
            }
            PieceDragSource::Reserve => {
                if piece_kind == PieceKind::Duck {
                    return Ok(Some(TurnInput::DragDrop(Turn::PlaceDuck(dest))));
                }
                Ok(Some(TurnInput::DragDrop(Turn::Drop(TurnDrop { piece_kind, to: dest }))))
            }
        }
    }

    // Pops one action from local action queue: a partial turn input, or a preturn.
    // Returns whether a preturn was cancelled.
    pub fn cancel_preturn(&mut self, board_idx: BughouseBoard) -> bool {
        if let Some((input_board_idx, _)) = self.partial_turn_input {
            if input_board_idx == board_idx {
                self.partial_turn_input = None;
                return false;
            }
        }

        if self.num_preturns_on_board(board_idx) == 0 {
            return false;
        }
        for (turn_idx, turn_record) in self.local_turns.iter().enumerate().rev() {
            if turn_record.envoy.board_idx == board_idx {
                self.local_turns.remove(turn_idx);
                return true;
            }
        }
        unreachable!(); // must have found a preturn, since num_preturns_on_board > 0
    }

    fn reset_local_changes(&mut self) {
        self.local_turns.clear();
        self.partial_turn_input = None;
    }

    fn cancel_partial_turn_on_board(&mut self, board_idx: BughouseBoard) {
        if let Some((input_board_idx, _)) = self.partial_turn_input {
            if input_board_idx == board_idx {
                self.partial_turn_input = None;
            }
        }
    }

    fn discard_invalid_local_turns(&mut self) {
        // Although we don't allow it currently, this function is written in a way that supports
        // turns cross-board turn dependencies.
        let mut game = self.game_confirmed.clone();
        let mut is_board_ok = enum_map! { _ => true };
        self.local_turns.retain(|turn_record| {
            let is_ok = game
                .turn_mode_for_envoy(turn_record.envoy)
                .and_then(|mode| game.apply_turn_record(turn_record, mode))
                .is_ok();
            if !is_ok {
                // Whenever we find an invalid turn, discard all subsequent turns on that board.
                is_board_ok[turn_record.envoy.board_idx] = false;
            }
            is_board_ok[turn_record.envoy.board_idx]
        });
        if self.apply_partial_turn(&mut game).is_err() {
            // Partial turn invalidated. Possible reasons: dragged piece was captured by opponent;
            // dragged piece depended on a (pre)turn that was cancelled.
            self.invalidate_partial_turn();
        }
    }

    fn game_with_local_turns(&self, local_turns_to_include: LocalTurns) -> BughouseGame {
        let (include_pre, include_partial) = match local_turns_to_include {
            LocalTurns::OnlyNormal => (false, false),
            LocalTurns::NormalAndPre => (true, false),
            LocalTurns::All => (true, true),
        };
        let mut game = self.game_confirmed.clone();
        for turn_record in self.local_turns.iter() {
            // Note. Not calling `test_flag`, because only server records flag defeat.
            // Unwrap ok: turn correctness (according to the `mode`) has already been verified.
            let mode = game.turn_mode_for_envoy(turn_record.envoy).unwrap();
            if mode == TurnMode::Preturn && !include_pre {
                // Do not break because we can still get in-order turns on the other board.
                continue;
            }
            game.apply_turn_record(turn_record, mode).unwrap();
        }
        if include_partial {
            // Unwrap ok: partial turn correctness has already been verified.
            self.apply_partial_turn(&mut game).unwrap();
        }
        game
    }

    pub fn num_preturns_on_board(&self, board_idx: BughouseBoard) -> usize {
        let mut game = self.game_confirmed.clone();
        let mut num_preturns = 0;
        for turn_record in self.local_turns.iter() {
            // Unwrap ok: turn correctness (according to the `mode`) has already been verified.
            let mode = game.turn_mode_for_envoy(turn_record.envoy).unwrap();
            if turn_record.envoy.board_idx == board_idx && mode == TurnMode::Preturn {
                num_preturns += 1;
            }
            game.apply_turn_record(turn_record, mode).unwrap();
        }
        num_preturns
    }

    fn apply_wayback_for_board(&self, game: &mut BughouseGame, board_idx: BughouseBoard) -> bool {
        let Some(ref turn_idx) = self.wayback_turn_index[board_idx] else {
            return false;
        };
        for turn in game.turn_log() {
            if turn.envoy.board_idx == board_idx && turn.index() >= *turn_idx {
                let turn_time = turn.time;
                let board_after = turn.board_after.clone();
                let board = game.board_mut(board_idx);
                *board = board_after;
                board.clock_mut().stop(turn_time);
                break;
            }
        }
        true
    }

    fn apply_wayback(&self, game: &mut BughouseGame) {
        for board_idx in BughouseBoard::iter() {
            self.apply_wayback_for_board(game, board_idx);
        }
    }

    fn try_partial_turn(
        &mut self, board_idx: BughouseBoard, input: PartialTurnInput,
    ) -> Result<(), ()> {
        self.partial_turn_input = Some((board_idx, input));
        let mut game = self.game_with_local_turns(LocalTurns::NormalAndPre);
        match self.apply_partial_turn(&mut game) {
            Ok(()) => Ok(()),
            Err(()) => {
                self.partial_turn_input = None;
                Err(())
            }
        }
    }

    fn apply_partial_turn(&self, game: &mut BughouseGame) -> Result<(), ()> {
        let Some((board_idx, input)) = self.partial_turn_input else {
            return Ok(());
        };
        let Some(envoy) = self.my_id.envoy_for(board_idx) else {
            return Err(());
        };
        match input {
            PartialTurnInput::Drag { piece_kind, piece_force, source } => {
                let board = game.board_mut(board_idx);
                match source {
                    PieceDragSource::Defunct => {}
                    PieceDragSource::Board(coord) => {
                        // Note: `take` modifies the board
                        let piece = board.grid_mut()[coord].take().ok_or(())?;
                        let expected = (piece_force, piece_kind);
                        let actual = (piece.force, piece.kind);
                        if expected != actual {
                            return Err(());
                        }
                    }
                    PieceDragSource::Reserve => {
                        let reserve = board.reserve_mut(envoy.force);
                        if reserve[piece_kind] == 0 {
                            return Err(());
                        }
                        reserve[piece_kind] -= 1;
                    }
                }
            }
            PartialTurnInput::StealPromotion { from, to } => {
                let Ok(mode) = game.turn_mode_for_envoy(envoy) else {
                    return Err(());
                };
                let board = game.board_mut(board_idx);
                if !board.is_legal_move_destination(from, to, mode) {
                    return Err(());
                }
                let grid = board.grid_mut();
                grid[to] = grid[from].take();
            }
        }
        Ok(())
    }

    fn invalidate_partial_turn(&mut self) {
        let Some((_, ref mut input)) = self.partial_turn_input else {
            return;
        };
        match input {
            PartialTurnInput::Drag { source, .. } => {
                *source = PieceDragSource::Defunct;
            }
            PartialTurnInput::StealPromotion { .. } => {
                self.partial_turn_input = None;
            }
        }
    }
}

fn board_turn_log_modulo_wayback(
    game: &BughouseGame, board_idx: BughouseBoard, wayback_turn_idx: Option<&str>,
) -> Vec<TurnRecordExpanded> {
    let mut turn_log = game
        .turn_log()
        .iter()
        .filter(|r| r.envoy.board_idx == board_idx)
        .cloned()
        .collect_vec();
    if let Some(wayback_turn_idx) = wayback_turn_idx {
        let mut wayback_turn_found = false;
        turn_log.retain(|r| {
            // The first turn with this condition should be kept, the rest should be deleted.
            if r.index().as_str() >= wayback_turn_idx {
                if !wayback_turn_found {
                    wayback_turn_found = true;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        });
    }
    turn_log
}

// Tuple values are compared lexicographically. Higher values overshadow lower values.
fn turn_highlight_z_index(highlight: &TurnHighlight) -> (u8, u8, u8) {
    (
        match highlight.layer {
            TurnHighlightLayer::AboveFog => 1,
            TurnHighlightLayer::BelowFog => 0,
        },
        match highlight.family {
            // Partial moves are the most important, because they help with the turn that user
            // inputs right now.
            TurnHighlightFamily::PartialTurn => 2,
            TurnHighlightFamily::Preturn => 1,
            TurnHighlightFamily::LatestTurn => 0,
        },
        match highlight.item {
            // Capture coincides with MoveTo (except for en-passant) and should take priority.
            // Whether MoveTo is above MoveFrom determines how sequences of preturns with a single
            // piece are rendered. The current approach if to highlight intermediate squares as
            // MoveTo, but this is a pretty arbitrary choice.
            TurnHighlightItem::Capture => 3,
            TurnHighlightItem::MoveTo => 2,
            TurnHighlightItem::Drop => 1,
            TurnHighlightItem::MoveFrom => 0,
        },
    )
}

fn make_turn_highlight(
    board_idx: BughouseBoard, coord: Coord, family: TurnHighlightFamily, item: TurnHighlightItem,
    fog_of_war_area: &HashSet<Coord>,
) -> Option<TurnHighlight> {
    // Highlights of all visible squares should be rendered below the fog. Semantically there is no
    // difference: the highlight will be visible anyway. But visually it's more appealing because it
    // doesn't obstruct the pieces and the edges of fog sprite extending from neighboring squares.
    let mut layer = TurnHighlightLayer::BelowFog;

    if fog_of_war_area.contains(&coord) {
        // Show the highlight in the fog when it doesn't give new information to the player.
        let show_in_fog = match (family, item) {
            // A piece owned by the current player before it was captured.
            (TurnHighlightFamily::LatestTurn, TurnHighlightItem::Capture) => true,
            // A turn made by the current player.
            (TurnHighlightFamily::Preturn | TurnHighlightFamily::PartialTurn, _) => true,
            // Default case: potentially new information.
            _ => false,
        };
        if show_in_fog {
            layer = TurnHighlightLayer::AboveFog;
        } else {
            return None;
        }
    }

    Some(TurnHighlight { board_idx, coord, layer, family, item })
}

fn get_turn_highlight_basis(turn_expanded: &TurnExpanded) -> Vec<(TurnHighlightItem, Coord)> {
    let mut highlights = vec![];
    if let Some((from, to)) = turn_expanded.relocation {
        highlights.push((TurnHighlightItem::MoveFrom, from));
        highlights.push((TurnHighlightItem::MoveTo, to));
    }
    if let Some((from, to)) = turn_expanded.relocation_extra {
        highlights.push((TurnHighlightItem::MoveFrom, from));
        highlights.push((TurnHighlightItem::MoveTo, to));
    }
    if let Some(drop) = turn_expanded.drop {
        highlights.push((TurnHighlightItem::Drop, drop));
    }
    for capture in turn_expanded.captures.iter() {
        if let Some(from) = capture.from {
            highlights.push((TurnHighlightItem::Capture, from));
        }
    }
    highlights
}

fn get_partial_turn_highlight_basis(
    partial_input: &PartialTurnInput,
) -> Vec<(TurnHighlightItem, Coord)> {
    match partial_input {
        PartialTurnInput::Drag { .. } => {
            // Highlighted separately. (Q. Should it?)
            vec![]
        }
        PartialTurnInput::StealPromotion { from, to } => vec![
            (TurnHighlightItem::MoveFrom, *from),
            (TurnHighlightItem::MoveTo, *to),
        ],
    }
}

fn expand_turn_highlights(
    basic_highlights: Vec<(TurnHighlightItem, Coord)>, family: TurnHighlightFamily,
    board_idx: BughouseBoard, fog_of_war_area: &HashSet<Coord>,
) -> Vec<TurnHighlight> {
    basic_highlights
        .into_iter()
        .filter_map(|(item, coord)| {
            make_turn_highlight(board_idx, coord, family, item, fog_of_war_area)
        })
        .collect()
}

fn get_turn_highlights(
    family: TurnHighlightFamily, board_idx: BughouseBoard, turn: &TurnExpanded,
    fog_of_war_area: &HashSet<Coord>,
) -> Vec<TurnHighlight> {
    expand_turn_highlights(get_turn_highlight_basis(turn), family, board_idx, fog_of_war_area)
}

fn get_partial_turn_highlights(
    board_idx: BughouseBoard, partial_input: &PartialTurnInput, fog_of_war_area: &HashSet<Coord>,
) -> Vec<TurnHighlight> {
    expand_turn_highlights(
        get_partial_turn_highlight_basis(partial_input),
        TurnHighlightFamily::PartialTurn,
        board_idx,
        fog_of_war_area,
    )
}
