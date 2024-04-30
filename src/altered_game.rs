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

use std::cmp;
use std::collections::HashSet;

use enum_map::enum_map;
use itertools::Itertools;
use strum::IntoEnumIterator;

use crate::board::{
    Board, PromotionTarget, Reachability, Turn, TurnDrop, TurnError, TurnExpanded, TurnInput,
    TurnMode, TurnMove,
};
use crate::clock::GameInstant;
use crate::coord::{BoardShape, Coord, SubjectiveRow};
use crate::display::Perspective;
use crate::force::Force;
use crate::game::{
    get_bughouse_force, BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus,
    BughouseParticipant, TurnIndex, TurnRecord, TurnRecordExpanded,
};
use crate::piece::{CastleDirection, PieceForce, PieceId, PieceKind, PieceOnBoard, PieceOrigin};
use crate::rules::{BughouseRules, ChessRules, Promotion};


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Location {
    Square(Coord),
    Reserve(Force, PieceKind),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TurnInputResult {
    Turn((BughouseBoard, TurnInput)),
    Noop,
    Error(TurnError),
}

#[derive(Clone, Copy, Debug)]
pub struct RegularPartialTurn {
    pub piece_kind: PieceKind,
    pub piece_force: PieceForce,
    pub piece_origin: PieceOrigin,
    pub source: PartialTurnSource,
}

#[derive(Clone, Copy, Debug)]
pub enum PartialTurnInput {
    Drag(RegularPartialTurn),
    ClickMove(RegularPartialTurn),
    UpgradePromotion { from: Coord, to: Coord },
    StealPromotion { from: Coord, to: Coord },
}

#[derive(Clone, Copy, Debug)]
pub enum PartialTurnSource {
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
    MoveFrom,         // piece moved from this square to another one
    MoveTo,           // piece moved from another square to this one
    Drop,             // piece dropped from reserve
    Capture,          // piece was captured here
    DragStart,        // drag start (while dragging a piece)
    LegalDestination, // legal moves (while dragging a piece)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SquareHighlight {
    pub board_idx: BughouseBoard,
    pub coord: Coord,
    pub layer: TurnHighlightLayer,
    pub family: TurnHighlightFamily,
    pub item: TurnHighlightItem,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReservePieceHighlight {
    pub board_idx: BughouseBoard,
    pub force: Force,
    pub piece_kind: PieceKind,
}

#[derive(Clone, Debug)]
pub struct TurnHighlights {
    pub square_highlights: Vec<SquareHighlight>,
    pub reserve_piece_highlights: Vec<ReservePieceHighlight>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LocalTurns {
    OnlyNormal,   // normal
    NormalAndPre, // normal + pre
    All,          // normal + pre + partial
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WaybackState {
    Disabled,           // cannot view old turns
    Enabled(TurnIndex), // can view old turns, but currently at the last turn
    Active(TurnIndex),  // viewing a historical turn
}

#[derive(Clone, Copy, Debug)]
pub enum WaybackDestination {
    Index(Option<TurnIndex>),
    Previous,
    Next,
    First,
    Last,
}

impl From<Result<(), TurnError>> for TurnInputResult {
    fn from(result: Result<(), TurnError>) -> Self {
        match result {
            Ok(()) => TurnInputResult::Noop,
            Err(e) => TurnInputResult::Error(e),
        }
    }
}

impl WaybackState {
    pub fn turn_index(&self) -> Option<TurnIndex> {
        match self {
            WaybackState::Disabled => None,
            WaybackState::Enabled(_) => None,
            WaybackState::Active(index) => Some(*index),
        }
    }
    pub fn display_turn_index(&self) -> Option<TurnIndex> {
        match self {
            WaybackState::Disabled => None,
            WaybackState::Enabled(index) => Some(*index),
            WaybackState::Active(index) => Some(*index),
        }
    }
    pub fn active(&self) -> bool { matches!(self, WaybackState::Active(_)) }
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
    wayback_turn_index: Option<TurnIndex>,
}

impl AlteredGame {
    pub fn new(my_id: BughouseParticipant, game_confirmed: BughouseGame) -> Self {
        AlteredGame {
            my_id,
            game_confirmed,
            partial_turn_input: None,
            local_turns: Vec::new(),
            wayback_turn_index: None,
        }
    }

    pub fn chess_rules(&self) -> &ChessRules { self.game_confirmed().chess_rules() }
    pub fn bughouse_rules(&self) -> &BughouseRules { self.game_confirmed().bughouse_rules() }
    pub fn board_shape(&self) -> BoardShape { self.game_confirmed().board_shape() }

    pub fn local_turns(&self) -> &[TurnRecord] { &self.local_turns }

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
    pub fn partial_turn_input_or_duck_turn(
        &self, board_idx: BughouseBoard,
    ) -> Option<PartialTurnInput> {
        let partial_turn = self.partial_turn_input.filter(|(b, _)| *b == board_idx).map(|(_, t)| t);
        let duck_turn = 'duck: {
            if !self.is_active() {
                break 'duck None;
            }
            let Some(envoy) = self.my_id.envoy_for(board_idx) else {
                break 'duck None;
            };
            let game = self.game_with_local_turns(LocalTurns::All);
            let board = game.board(board_idx);
            if !board.is_duck_turn(envoy.force) {
                break 'duck None;
            }
            Some(PartialTurnInput::ClickMove(RegularPartialTurn {
                piece_kind: PieceKind::Duck,
                piece_force: PieceForce::Neutral,
                piece_origin: PieceOrigin::Innate,
                source: board
                    .duck_position()
                    .map_or(PartialTurnSource::Reserve, |pos| PartialTurnSource::Board(pos)),
            }))
        };
        partial_turn.or(duck_turn)
    }

    // TODO: Avoid copying the game for it.
    pub fn is_my_duck_turn(&self, board_idx: BughouseBoard) -> bool {
        if !self.is_active() {
            return false;
        }
        let Some(envoy) = self.my_id.envoy_for(board_idx) else {
            return false;
        };
        let game = self.game_with_local_turns(LocalTurns::All);
        game.board(board_idx).is_duck_turn(envoy.force)
    }

    pub fn turn_highlights(&self) -> TurnHighlights {
        let my_id = self.my_id;
        let game = self.local_game();
        let mut square_highlights = vec![];
        let mut reserve_piece_highlights = vec![];
        let wayback = self.wayback_state();
        let wayback_turn_idx = wayback.display_turn_index();

        for board_idx in BughouseBoard::iter() {
            if wayback.active() {
                let wayback_turn_board =
                    game.turn_record(wayback_turn_idx.unwrap()).envoy.board_idx;
                if wayback_turn_board != board_idx {
                    continue;
                }
            }

            let board = game.board(board_idx);
            let see_though_fog = self.see_though_fog();
            let empty_area = HashSet::new();
            let fog_render_area = self.fog_of_war_area(board_idx);
            let fog_cover_area = if see_though_fog { &empty_area } else { &fog_render_area };
            let turn_log = board_turn_log_modulo_wayback(&game, board_idx, wayback_turn_idx);

            if let Some(latest_turn_record) = turn_log.last() {
                if !my_id.plays_for(latest_turn_record.envoy) || wayback_turn_idx.is_some() {
                    // Highlight all components of the latest megaturn. Normally this would be
                    // exactly one turn, but in duck chess it's both the regular piece and the duck.
                    for r in
                        turn_log.iter().rev().take_while(|r| r.envoy == latest_turn_record.envoy)
                    {
                        square_highlights.extend(get_turn_highlights(
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
                    square_highlights.extend(get_turn_highlights(
                        TurnHighlightFamily::Preturn,
                        board_idx,
                        &r.turn_expanded,
                        fog_cover_area,
                    ));
                }
            }

            if let Some(partial_input) = self.partial_turn_input_or_duck_turn(board_idx) {
                square_highlights.extend(get_partial_turn_highlights(
                    board_idx,
                    partial_input,
                    board,
                    fog_cover_area,
                ));
                reserve_piece_highlights
                    .extend(get_partial_turn_reserve_highlights(board_idx, partial_input));
            }
        }
        // Note: Don't use `group_by`: it only groups consecutive elements.
        let square_highlights = square_highlights
            .into_iter()
            .into_group_map_by(|h| (h.board_idx, h.coord))
            .into_iter()
            .flat_map(|(_, group)| {
                group
                    .into_iter()
                    .sorted_by_key(|h| cmp::Reverse(turn_highlight_z_index(h)))
                    .take_while_inclusive(|h| !is_turn_highlight_opaque(h.item))
            })
            .collect();
        TurnHighlights {
            square_highlights,
            reserve_piece_highlights,
        }
    }

    pub fn see_though_fog(&self) -> bool { !self.is_active() }

    pub fn fog_of_war_area(&self, board_idx: BughouseBoard) -> HashSet<Coord> {
        if !self.chess_rules().fog_of_war {
            return HashSet::new();
        }

        // Don't use `local_game`: preturns and drags should not reveal new areas. Even this logic
        // is not 100% safe: in some game variants local in-order turns could be reverted, e.g. when
        // a piece is stolen after a pawn promotion or when you receive a king in koedem. In such
        // cases you would be able to briefly see areas that shouldn't have been revealed. But such
        // occasions are rare, so it seem better than waiting for server response in order to lift
        // the fog. The latter would feel laggy.
        let mut game = self.game_with_local_turns(LocalTurns::OnlyNormal);

        let board_shape = self.board_shape();
        let wayback_active = self.apply_wayback(&mut game);
        let force = match self.my_id {
            BughouseParticipant::Player(id) => get_bughouse_force(id.team(), board_idx),
            BughouseParticipant::Observer(_) => game.board(board_idx).active_force(),
        };
        let mut visible = game.board(board_idx).fog_free_area(force);
        // Still, do show preturn pieces themselves:
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
    }

    pub fn try_local_turn(
        &mut self, board_idx: BughouseBoard, turn_input: TurnInput, time: GameInstant,
    ) -> Result<TurnMode, TurnError> {
        let Some(envoy) = self.my_id.envoy_for(board_idx) else {
            return Err(TurnError::NotPlayer);
        };
        if self.wayback_turn_index.is_some() {
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

    pub fn wayback_state(&self) -> WaybackState {
        if self.is_active() {
            WaybackState::Disabled
        } else if let Some(index) = self.wayback_turn_index {
            WaybackState::Active(index)
        } else {
            if let Some(last_turn) = self.game_confirmed.last_turn_record() {
                WaybackState::Enabled(last_turn.index)
            } else {
                WaybackState::Disabled
            }
        }
    }
    // Navigates game history allowing to view historical state. `board_idx` allows to navigate to
    // the previous/next/first/last turn on a specific board; it is ignored when `destination` is
    // `Index`.
    pub fn wayback_to(
        &mut self, destination: WaybackDestination, board_idx: Option<BughouseBoard>,
    ) -> Option<TurnIndex> {
        assert!(!self.is_active());
        let Some(old_index) = self
            .wayback_turn_index
            .or_else(|| self.game_confirmed.last_turn_record().map(|r| r.index))
        else {
            return None;
        };
        let mut iter = self
            .game_confirmed
            .turn_log()
            .iter()
            .filter(move |r| board_idx.map_or(true, |b| r.envoy.board_idx == b))
            .map(|r| r.index);
        let new_index = match destination {
            WaybackDestination::Index(index) => index,
            WaybackDestination::Previous => {
                // Going to the previous turn is a bit peculiar. A simple implementation would be
                //   iter_rev.find_or_last(|index| *index <= old_index)
                // It works fine when `board_idx` is `None` or when the current turn is already on
                // the target board. But when the current turn is on the other board, it find the
                // largest smaller turn on the target board, which is the turn that the user already
                // sees. Hence the custom logic to make sure we actually update the target board.
                let mut iter_rev = iter.clone().rev();
                let candidate = match iter_rev.find(|index| *index <= old_index) {
                    Some(local_index) => iter_rev.find(|index| *index < local_index),
                    None => iter_rev.last(),
                };
                candidate.or(iter.nth(0))
            }
            WaybackDestination::Next => iter.find_or_last(|index| *index > old_index),
            WaybackDestination::First => iter.nth(0),
            WaybackDestination::Last => iter.rev().nth(0),
        };
        let at_end = new_index == self.game_confirmed.last_turn_record().map(|r| r.index);
        self.wayback_turn_index = if at_end { None } else { new_index };
        self.wayback_turn_index
    }

    pub fn choose_promotion_upgrade(&mut self, piece_kind: PieceKind) -> TurnInputResult {
        if let Some((input_board_idx, partial_input)) = self.partial_turn_input {
            match partial_input {
                PartialTurnInput::Drag { .. } => {}
                PartialTurnInput::ClickMove { .. } => {}
                PartialTurnInput::UpgradePromotion { from, to } => {
                    let full_input = TurnInput::DragDrop(Turn::Move(TurnMove {
                        from,
                        to,
                        promote_to: Some(PromotionTarget::Upgrade(piece_kind)),
                    }));
                    self.partial_turn_input = None;
                    return TurnInputResult::Turn((input_board_idx, full_input));
                }
                PartialTurnInput::StealPromotion { .. } => {}
            }
        }
        TurnInputResult::Noop
    }

    // Improvement: Less ad-hoc solution for "gluing" board index to TurnInput; use it here, in
    // `drag_piece_drop` and in `BughouseClientEvent::MakeTurn`.
    pub fn click(&mut self, board_idx: BughouseBoard, loc: Location) -> TurnInputResult {
        if self.wayback_turn_index.is_some() {
            return TurnInputResult::Error(TurnError::WaybackIsActive);
        }
        if let Some((input_board_idx, partial_input)) = self.partial_turn_input {
            match partial_input {
                PartialTurnInput::Drag { .. } => {
                    return TurnInputResult::Error(TurnError::PreviousTurnNotFinished)
                }
                PartialTurnInput::ClickMove(regular_partial_turn) => {
                    match loc {
                        Location::Reserve(force, piece_kind) => {
                            if regular_partial_turn.piece_force.is_owned_by_or_neutral(force)
                                && piece_kind == regular_partial_turn.piece_kind
                            {
                                // Deselect reserve pieces by clicking again. Note that we don't do
                                // this for regular pieces because the duck can be premoved back to
                                // the original place.
                                self.partial_turn_input = None;
                                return TurnInputResult::Noop;
                            }
                        }
                        Location::Square(coord) => {
                            if board_idx == input_board_idx {
                                return self.apply_destination_to_regular_partial_turn(
                                    regular_partial_turn,
                                    board_idx,
                                    coord,
                                );
                            }
                        }
                    }
                    self.partial_turn_input = None;
                    // Fallthrough: begin new move.
                }
                PartialTurnInput::UpgradePromotion { .. } => {
                    return TurnInputResult::Noop;
                }
                PartialTurnInput::StealPromotion { from, to } => {
                    if board_idx == input_board_idx.other()
                        && let Location::Square(coord) = loc
                    {
                        let game = self.game_with_local_turns(LocalTurns::All);
                        if let Some(piece) = game.board(board_idx).grid()[coord] {
                            let full_input = TurnInput::DragDrop(Turn::Move(TurnMove {
                                from,
                                to,
                                promote_to: Some(PromotionTarget::Steal((
                                    piece.kind,
                                    piece.origin,
                                    piece.id,
                                ))),
                            }));
                            self.partial_turn_input = None;
                            return TurnInputResult::Turn((input_board_idx, full_input));
                        }
                    }
                    return TurnInputResult::Noop;
                }
            }
        }
        if self.is_my_duck_turn(board_idx) {
            match loc {
                Location::Square(coord) => {
                    TurnInputResult::Turn((board_idx, TurnInput::DragDrop(Turn::PlaceDuck(coord))))
                }
                Location::Reserve(..) => TurnInputResult::Noop,
            }
        } else {
            match loc {
                Location::Square(coord) => {
                    if let Some(piece) =
                        self.game_with_local_turns(LocalTurns::All).board(board_idx).grid()[coord]
                    {
                        self.try_partial_turn(
                            board_idx,
                            PartialTurnInput::ClickMove(RegularPartialTurn {
                                piece_kind: piece.kind,
                                piece_force: piece.force,
                                piece_origin: piece.origin,
                                source: PartialTurnSource::Board(coord),
                            }),
                        )
                        .into()
                    } else {
                        TurnInputResult::Noop
                    }
                }
                Location::Reserve(force, piece_kind) => {
                    let piece_force = piece_kind.reserve_piece_force(force);
                    self.try_partial_turn(
                        board_idx,
                        PartialTurnInput::ClickMove(RegularPartialTurn {
                            piece_kind,
                            piece_force,
                            piece_origin: PieceOrigin::Dropped,
                            source: PartialTurnSource::Reserve,
                        }),
                    )
                    .into()
                }
            }
        }
    }

    pub fn piece_drag_state(&self) -> PieceDragState {
        match self.partial_turn_input {
            Some((_, PartialTurnInput::Drag(RegularPartialTurn { source, .. }))) => match source {
                PartialTurnSource::Defunct => PieceDragState::Defunct,
                _ => PieceDragState::Dragging,
            },
            _ => PieceDragState::NoDrag,
        }
    }

    pub fn start_drag_piece(
        &mut self, board_idx: BughouseBoard, loc: Location,
    ) -> Result<(), TurnError> {
        if self.wayback_turn_index.is_some() {
            return Err(TurnError::WaybackIsActive);
        }
        if let Some((input_board_idx, _)) = self.partial_turn_input {
            if input_board_idx == board_idx {
                return Err(TurnError::PreviousTurnNotFinished);
            }
        }
        self.partial_turn_input = None;
        let game = self.game_with_local_turns(LocalTurns::All);
        let board = game.board(board_idx);
        let (piece_kind, piece_force, piece_origin, source) = match loc {
            Location::Square(coord) => {
                let piece = board.grid()[coord].ok_or(TurnError::PieceMissing)?;
                (piece.kind, piece.force, piece.origin, PartialTurnSource::Board(coord))
            }
            Location::Reserve(force, piece_kind) => {
                let piece_force = piece_kind.reserve_piece_force(force);
                (piece_kind, piece_force, PieceOrigin::Dropped, PartialTurnSource::Reserve)
            }
        };
        self.try_partial_turn(
            board_idx,
            PartialTurnInput::Drag(RegularPartialTurn {
                piece_kind,
                piece_force,
                piece_origin,
                source,
            }),
        )
    }

    pub fn abort_drag_piece(&mut self) {
        if matches!(self.partial_turn_input, Some((_, PartialTurnInput::Drag { .. }))) {
            self.partial_turn_input = None;
        }
    }

    // Stop drag and returns turn on success. The client should then manually apply this
    // turn via `make_turn`.
    pub fn drag_piece_drop(&mut self, board_idx: BughouseBoard, dest: Coord) -> TurnInputResult {
        if let Some((input_board_idx, PartialTurnInput::Drag(regular_partial_turn))) =
            self.partial_turn_input
        {
            if input_board_idx != board_idx {
                // TODO: Log internal error: web client should make sure that only one board is
                // interactive while dragging.
                return TurnInputResult::Error(TurnError::NoTurnInProgress);
            }
            self.apply_destination_to_regular_partial_turn(regular_partial_turn, board_idx, dest)
        } else {
            TurnInputResult::Error(TurnError::NoTurnInProgress)
        }
    }

    pub fn highlight_square_on_hover(&self, board_idx: BughouseBoard) -> bool {
        if let Some((input_board_idx, partial_input)) = self.partial_turn_input {
            input_board_idx == board_idx && matches!(partial_input, PartialTurnInput::ClickMove(_))
        } else {
            self.is_my_duck_turn(board_idx)
        }
    }

    pub fn apply_destination_to_regular_partial_turn(
        &mut self, regular_partial_turn: RegularPartialTurn, board_idx: BughouseBoard, dest: Coord,
    ) -> TurnInputResult {
        let make_turn = |turn_input| TurnInputResult::Turn((board_idx, turn_input));

        let RegularPartialTurn {
            piece_kind,
            piece_force,
            piece_origin,
            source,
        } = regular_partial_turn;
        self.partial_turn_input = None;

        match source {
            PartialTurnSource::Defunct => TurnInputResult::Error(TurnError::Defunct),
            PartialTurnSource::Board(source_coord) => {
                use PieceKind::*;
                if piece_kind == PieceKind::Duck {
                    return make_turn(TurnInput::DragDrop(Turn::PlaceDuck(dest)));
                }
                if source_coord == dest {
                    return TurnInputResult::Error(TurnError::Cancelled);
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
                    if piece_origin != PieceOrigin::Innate {
                        return TurnInputResult::Error(TurnError::CannotCastleDroppedKing);
                    }
                    let dir = if d_col > 0 { HSide } else { ASide };
                    make_turn(TurnInput::DragDrop(Turn::Castle(dir)))
                } else {
                    if is_promotion {
                        match self.bughouse_rules().promotion {
                            Promotion::Upgrade => self
                                .try_partial_turn(board_idx, PartialTurnInput::UpgradePromotion {
                                    from: source_coord,
                                    to: dest,
                                })
                                .into(),
                            Promotion::Discard => {
                                make_turn(TurnInput::DragDrop(Turn::Move(TurnMove {
                                    from: source_coord,
                                    to: dest,
                                    promote_to: Some(PromotionTarget::Discard),
                                })))
                            }
                            Promotion::Steal => self
                                .try_partial_turn(board_idx, PartialTurnInput::StealPromotion {
                                    from: source_coord,
                                    to: dest,
                                })
                                .into(),
                        }
                    } else {
                        make_turn(TurnInput::DragDrop(Turn::Move(TurnMove {
                            from: source_coord,
                            to: dest,
                            promote_to: None,
                        })))
                    }
                }
            }
            PartialTurnSource::Reserve => {
                if piece_kind == PieceKind::Duck {
                    return make_turn(TurnInput::DragDrop(Turn::PlaceDuck(dest)));
                }
                make_turn(TurnInput::DragDrop(Turn::Drop(TurnDrop { piece_kind, to: dest })))
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

    fn apply_wayback(&self, game: &mut BughouseGame) -> bool {
        let Some(ref turn_idx) = self.wayback_turn_index else {
            return false;
        };
        let turn = game.turn_log().iter().rev().find(|turn| turn.index <= *turn_idx).unwrap();
        let turn_time = turn.time;
        let boards_after = turn.boards_after.clone();
        for (board_idx, mut board) in boards_after {
            board.clock_mut().stop(turn_time);
            *game.board_mut(board_idx) = board;
        }
        true
    }

    fn try_partial_turn(
        &mut self, board_idx: BughouseBoard, input: PartialTurnInput,
    ) -> Result<(), TurnError> {
        self.partial_turn_input = Some((board_idx, input));
        let mut game = self.game_with_local_turns(LocalTurns::NormalAndPre);
        let result = self.apply_partial_turn(&mut game);
        if result.is_err() {
            self.partial_turn_input = None;
        }
        result
    }

    fn apply_partial_turn(&self, game: &mut BughouseGame) -> Result<(), TurnError> {
        let Some((board_idx, input)) = self.partial_turn_input else {
            return Ok(());
        };
        let Some(envoy) = self.my_id.envoy_for(board_idx) else {
            return Err(TurnError::NotPlayer);
        };
        let is_drag = matches!(input, PartialTurnInput::Drag(_));
        match input {
            PartialTurnInput::Drag(input) | PartialTurnInput::ClickMove(input) => {
                if !input.piece_force.is_owned_by_or_neutral(envoy.force) {
                    return Err(TurnError::DontControlPiece);
                }
                let board = game.board_mut(board_idx);
                match input.source {
                    PartialTurnSource::Defunct => {}
                    PartialTurnSource::Board(coord) => {
                        let piece = board.grid()[coord].ok_or(TurnError::PieceMissing)?;
                        let expected = (input.piece_force, input.piece_kind, input.piece_origin);
                        let actual = (piece.force, piece.kind, piece.origin);
                        if expected != actual {
                            return Err(TurnError::TurnObsolete);
                        }
                        if is_drag {
                            board.grid_mut()[coord] = None;
                        }
                    }
                    PartialTurnSource::Reserve => {
                        if !input.piece_force.is_owned_by_or_neutral(envoy.force) {
                            return Err(TurnError::DontControlPiece);
                        }
                        let reserve = board.reserve_mut(envoy.force);
                        if reserve[input.piece_kind] == 0 {
                            return Err(TurnError::DropPieceMissing);
                        }
                        if is_drag {
                            reserve[input.piece_kind] -= 1;
                        }
                    }
                }
            }
            PartialTurnInput::UpgradePromotion { from, to }
            | PartialTurnInput::StealPromotion { from, to } => {
                let mode = game.turn_mode_for_envoy(envoy)?;
                let board = game.board_mut(board_idx);
                match board.destination_reachability(from, to, mode) {
                    Reachability::Reachable => {}
                    Reachability::Blocked => return Err(TurnError::PathBlocked),
                    Reachability::Impossible => return Err(TurnError::ImpossibleTrajectory),
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
            // Note the difference between `Drag` and `ClickMove`: immediately vanishing dragged
            // piece breaks the feeling of physicality (so we use `Defunct` here), but aborting a
            // click move feels fine.
            PartialTurnInput::Drag(RegularPartialTurn { source, .. }) => {
                *source = PartialTurnSource::Defunct;
            }
            PartialTurnInput::ClickMove { .. }
            | PartialTurnInput::UpgradePromotion { .. }
            | PartialTurnInput::StealPromotion { .. } => {
                self.partial_turn_input = None;
            }
        }
    }
}

fn board_turn_log_modulo_wayback(
    game: &BughouseGame, board_idx: BughouseBoard, wayback_turn_idx: Option<TurnIndex>,
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
            if r.index >= wayback_turn_idx {
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
fn turn_highlight_z_index(highlight: &SquareHighlight) -> (u8, u8, u8) {
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
            TurnHighlightItem::LegalDestination => 11,
            TurnHighlightItem::DragStart => 10,
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

// For each square, we'll show the top-most opaque highlight plus all non-opaque highlights above.
fn is_turn_highlight_opaque(item: TurnHighlightItem) -> bool {
    use TurnHighlightItem::*;
    match item {
        LegalDestination | DragStart => false,
        Capture | MoveTo | Drop | MoveFrom => true,
    }
}

fn make_turn_highlight(
    board_idx: BughouseBoard, coord: Coord, family: TurnHighlightFamily, item: TurnHighlightItem,
    fog_of_war_area: &HashSet<Coord>,
) -> Option<SquareHighlight> {
    // Opaque highlights of all visible squares should be rendered below the fog. Semantically there
    // is no difference: the highlight will be visible anyway. But visually it's more appealing
    // because it doesn't obstruct the pieces and the edges of fog sprite extending from neighboring
    // squares.
    let mut layer = if is_turn_highlight_opaque(item) {
        TurnHighlightLayer::BelowFog
    } else {
        TurnHighlightLayer::AboveFog
    };

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

    Some(SquareHighlight { board_idx, coord, layer, family, item })
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
    if let Turn::PlaceDuck(coord) = turn_expanded.turn {
        highlights.push((TurnHighlightItem::MoveTo, coord));
    } else if let Some(drop) = turn_expanded.drop {
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
    partial_input: PartialTurnInput, board: &Board,
) -> Vec<(TurnHighlightItem, Coord)> {
    use PieceKind::*;
    let add_legal_moves = |input: RegularPartialTurn, from: Coord, highlights: &mut Vec<_>| {
        let mut board = board.clone();
        board.grid_mut()[from] = Some(PieceOnBoard {
            id: PieceId::tmp(),
            kind: input.piece_kind,
            force: input.piece_force,
            origin: input.piece_origin,
        });
        // Add move hints to fairy pieces which the player may be unfamiliar with.
        let need_move_hint = match input.piece_kind {
            Pawn | Knight | Bishop | Rook | Queen | King => false,
            Cardinal | Empress | Amazon => true,
            Duck => false,
        };
        // Disable legal move destination hints in fog of war: highlighting all reachable
        // squares would give away information. Not highlighting them could be misleading.
        let fog_of_war = board.chess_rules().fog_of_war;
        if need_move_hint && !fog_of_war {
            for dest in board.legal_turn_destinations(from) {
                highlights.push((TurnHighlightItem::LegalDestination, dest));
            }
        }
    };
    match partial_input {
        PartialTurnInput::Drag(input) => {
            let mut highlights = vec![];
            let from = match input.source {
                PartialTurnSource::Defunct => return vec![],
                PartialTurnSource::Board(coord) => coord,
                PartialTurnSource::Reserve => return vec![],
            };
            highlights.push((TurnHighlightItem::DragStart, from));
            add_legal_moves(input, from, &mut highlights);
            highlights
        }
        PartialTurnInput::ClickMove(input) => {
            let mut highlights = vec![];
            let from = match input.source {
                PartialTurnSource::Defunct => return vec![],
                PartialTurnSource::Board(coord) => coord,
                // Handled by `get_partial_turn_reserve_highlights`.
                PartialTurnSource::Reserve => return vec![],
            };
            highlights.push((TurnHighlightItem::MoveFrom, from));
            add_legal_moves(input, from, &mut highlights);
            highlights
        }
        PartialTurnInput::StealPromotion { from, to }
        | PartialTurnInput::UpgradePromotion { from, to } => vec![
            (TurnHighlightItem::MoveFrom, from),
            (TurnHighlightItem::MoveTo, to),
        ],
    }
}

fn expand_turn_highlights(
    basic_highlights: Vec<(TurnHighlightItem, Coord)>, family: TurnHighlightFamily,
    board_idx: BughouseBoard, fog_of_war_area: &HashSet<Coord>,
) -> Vec<SquareHighlight> {
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
) -> Vec<SquareHighlight> {
    expand_turn_highlights(get_turn_highlight_basis(turn), family, board_idx, fog_of_war_area)
}

fn get_partial_turn_highlights(
    board_idx: BughouseBoard, partial_input: PartialTurnInput, board: &Board,
    fog_of_war_area: &HashSet<Coord>,
) -> Vec<SquareHighlight> {
    expand_turn_highlights(
        get_partial_turn_highlight_basis(partial_input, board),
        TurnHighlightFamily::PartialTurn,
        board_idx,
        fog_of_war_area,
    )
}

fn get_partial_turn_reserve_highlights(
    board_idx: BughouseBoard, partial_input: PartialTurnInput,
) -> Vec<ReservePieceHighlight> {
    match partial_input {
        PartialTurnInput::ClickMove(input) => match input.source {
            PartialTurnSource::Defunct => vec![],
            PartialTurnSource::Board(_) => vec![],
            PartialTurnSource::Reserve => {
                use PieceKind::*;
                let force =
                    input.piece_force.try_into().unwrap_or_else(|_| match input.piece_kind {
                        Duck => Force::White, // before the first move
                        Pawn | Knight | Bishop | Rook | Queen | Cardinal | Empress | Amazon
                        | King => panic!("Piece should never be neutral: {:?}", input.piece_kind),
                    });
                vec![ReservePieceHighlight {
                    board_idx,
                    force,
                    piece_kind: input.piece_kind,
                }]
            }
        },
        PartialTurnInput::Drag(_)
        | PartialTurnInput::UpgradePromotion { .. }
        | PartialTurnInput::StealPromotion { .. } => vec![],
    }
}
