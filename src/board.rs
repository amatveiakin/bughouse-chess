// Improvement potential. Chess draws: dead position, stalemate, fifty-move rule.

#![allow(unused_parens)]

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::rc::Rc;

use enum_map::{enum_map, EnumMap};
use itertools::{iproduct, Itertools};
use serde::{Deserialize, Serialize};

use crate::algebraic::{
    AlgebraicDetails, AlgebraicDrop, AlgebraicMove, AlgebraicPromotionTarget, AlgebraicTurn,
};
use crate::clock::{Clock, GameInstant};
use crate::coord::{BoardShape, Col, Coord, Row, SubjectiveRow};
use crate::force::Force;
use crate::grid::{Grid, GridForRepetitionDraw};
use crate::piece::{
    accolade_combine_pieces, CastleDirection, PieceForRepetitionDraw, PieceForce, PieceId,
    PieceKind, PieceMovement, PieceOnBoard, PieceOrigin, PieceReservable,
};
use crate::rules::{
    BughouseRules, ChessRules, DropAggression, FairyPieces, MatchRules, Promotion, Rules,
};
use crate::starter::{
    generate_starting_grid, starting_piece_row, BoardSetup, EffectiveStartingPosition,
};
use crate::util::sort_two;


fn tuple_abs((a, b): (i8, i8)) -> (u8, u8) {
    (a.abs().try_into().unwrap(), b.abs().try_into().unwrap())
}

fn apply_sign(value: u8, sign_source: i8) -> Option<i8> {
    if (value == 0) != (sign_source == 0) {
        // This is used by reachability computation to find in which direction to move.
        // If "from" and "to" squares are in the same row, but the piece always changes row
        // when moving, then there's obviously no chance to find a valid move. And vice versa.
        // Same for cols.
        None
    } else {
        Some((value as i8) * sign_source.signum())
    }
}

fn iter_minmax<T: PartialOrd + Copy, I: Iterator<Item = T>>(iter: I) -> Option<(T, T)> {
    match iter.minmax() {
        itertools::MinMaxResult::NoElements => None,
        itertools::MinMaxResult::OneElement(v) => Some((v, v)),
        itertools::MinMaxResult::MinMax(min, max) => Some((min, max)),
    }
}

fn direction_forward(force: Force) -> i8 {
    match force {
        Force::White => 1,
        Force::Black => -1,
    }
}

fn col_range_inclusive((col_min, col_max): (Col, Col)) -> impl Iterator<Item = Col> {
    assert!(col_min <= col_max);
    (col_min.to_zero_based()..=col_max.to_zero_based()).map(Col::from_zero_based)
}

fn combine_pieces(
    rules: &ChessRules, id: PieceId, first: PieceOnBoard, second: PieceOnBoard,
) -> Option<PieceOnBoard> {
    match rules.fairy_pieces {
        FairyPieces::NoFairy => None,
        FairyPieces::Accolade => accolade_combine_pieces(id, first, second),
    }
}

fn find_piece(grid: &Grid, predicate: impl Fn(PieceOnBoard) -> bool) -> Option<Coord> {
    let mut coord = None;
    for pos in grid.shape().coords() {
        if let Some(piece) = grid[pos] {
            if predicate(piece) {
                match coord {
                    None => coord = Some(pos),
                    Some(_) => return None,
                }
            }
        }
    }
    coord
}

fn find_piece_by_id(grid: &Grid, id: PieceId) -> Option<Coord> { find_piece(grid, |p| p.id == id) }

// This assumes that there is exactly one king, so it should never be used if `chess_rules.regicide`
// is true. Also be careful with preturns for the same reason.
fn find_king(grid: &Grid, force: Force) -> Option<Coord> {
    find_piece(grid, |p| p.kind == PieceKind::King && p.force == force.into())
}

fn should_promote(board_shape: BoardShape, force: Force, piece_kind: PieceKind, to: Coord) -> bool {
    let last_row = SubjectiveRow::last(board_shape).to_row(board_shape, force);
    piece_kind == PieceKind::Pawn && to.row == last_row
}

fn can_capture(attacker_force: PieceForce, target_force: PieceForce) -> bool {
    use PieceForce::*;
    match (attacker_force, target_force) {
        (White, Black) | (Black, White) => true,
        (White, White) | (Black, Black) => false,
        // Duck cannot be captured. Could require checking piece kind if other neutral pieces
        // are introduced.
        (_, Neutral) | (Neutral, _) => false,
    }
}

fn get_capture(
    grid: &Grid, from: Coord, to: Coord, en_passant_target: Option<Coord>,
) -> Option<Coord> {
    let piece = grid[from].unwrap();
    if let Some(target_piece) = grid[to] {
        if can_capture(piece.force, target_piece.force) {
            return Some(to);
        } else {
            return None;
        }
    } else if let Some(en_passant_target) = en_passant_target {
        if piece.kind == PieceKind::Pawn && to == en_passant_target {
            if let Ok(force) = Force::try_from(piece.force) {
                let row = en_passant_target.row + direction_forward(force.opponent());
                return Some(Coord::new(row, en_passant_target.col));
            }
        }
    }
    None
}

fn get_en_passant_target(grid: &Grid, turn: Turn) -> Option<Coord> {
    if let Turn::Move(mv) = turn {
        let piece_kind = grid[mv.to]?.kind;
        if piece_kind == PieceKind::Pawn
            && mv.to.col == mv.from.col
            && (mv.to.row - mv.from.row).abs() == 2
        {
            let row_idx = (mv.to.row.to_zero_based() + mv.from.row.to_zero_based()) / 2;
            let row = Row::from_zero_based(row_idx);
            return Some(Coord::new(row, mv.to.col));
        }
    }
    None
}

// Shows which squares are revealed by a given piece in fog-of-war variant.
//
// Similar to `legal_move_destinations`, but treats pawns differently:
//   - When there is a piece in front of a pawn, the piece is not shown, because that would
//     be new information that you arguably shouldn't have.
//   - When there is no piece diagonally from a pawn, the square not reachable but revealed.
//     Reasoning: the fact that a pawn cannot capture implies that the square is empty, thus
//     showing it gives no new information and is a purely visual change. (Ok, technically,
//     it's not 100% visual. It could potentially reduce information, because you no longer
//     see en passant opportunities.)
fn visibility_from(
    rules: &ChessRules, grid: &Grid, from: Coord, en_passant_target: Option<Coord>,
) -> Vec<Coord> {
    // Improvement potential: Don't iterate over all squares.
    grid.shape()
        .coords()
        .filter(|&to| {
            let capture = get_capture(grid, from, to, en_passant_target);
            let capturing = if capture.is_some() {
                Capturing::Yes
            } else {
                Capturing::Maybe
            };
            generic_reachability(rules, grid, from, to, capturing).ok()
        })
        .collect()
}

fn legal_move_destinations(
    rules: &ChessRules, grid: &Grid, from: Coord, en_passant_target: Option<Coord>,
) -> Vec<Coord> {
    // Improvement potential: Don't iterate over all squares.
    grid.shape()
        .coords()
        .filter(|&to| {
            let capture = get_capture(grid, from, to, en_passant_target);
            reachability(rules, grid, from, to, capture.is_some()).ok()
        })
        .collect()
}

// Generates two kinds of castling moves: moving the king onto a neighboring rook and moving the
// king two cols in the corresponding direction. In reality it's also possible to move the king
// by 3 or more cols, but we exclude this to reduce clutter.
// TODO: Exclude moves when the path is blocked.
fn legal_castling_destinations(
    grid: &Grid, from: Coord, castling_rights: &EnumMap<Force, CastlingRights>,
) -> Vec<Coord> {
    let Some(piece) = grid[from] else {
        return vec![];
    };
    if piece.kind != PieceKind::King {
        return vec![];
    }
    let Ok(force) = Force::try_from(piece.force) else {
        return vec![];
    };
    let mut dst_cols = vec![];
    for (dir, rook_col) in castling_rights[force] {
        if let Some(rook_col) = rook_col {
            if (rook_col - from.col).abs() == 1 {
                dst_cols.push(rook_col);
            }
            let d = match dir {
                CastleDirection::ASide => -2,
                CastleDirection::HSide => 2,
            };
            let jump_col = from.col + d;
            if grid.contains_col(jump_col) {
                dst_cols.push(jump_col);
            }
        }
    }
    dst_cols.sort();
    dst_cols.dedup();
    dst_cols.into_iter().map(|col| Coord::new(from.row, col)).collect()
}

fn king_force(grid: &Grid, king_pos: Coord) -> Force {
    let piece = grid[king_pos].unwrap();
    assert_eq!(piece.kind, PieceKind::King);
    piece.force.try_into().unwrap()
}

// Grid is guaratneed to be returned intact.
fn is_chess_mate_to(
    rules: &ChessRules, grid: &mut Grid, king_pos: Coord, en_passant_target: Option<Coord>,
) -> bool {
    if !is_check_to(rules, grid, king_pos) {
        return false;
    }
    let force = king_force(grid, king_pos);
    for from in grid.shape().coords() {
        if let Some(piece) = grid[from] {
            if piece.force == force.into() {
                for to in legal_move_destinations(rules, grid, from, en_passant_target) {
                    let capture_or = get_capture(grid, from, to, en_passant_target);
                    // Zero out capture separately because of en passant.
                    let mut grid = grid.maybe_scoped_set(capture_or.map(|pos| (pos, None)));
                    let mut grid = grid.scoped_set(from, None);
                    let grid = grid.scoped_set(to, Some(piece));
                    let new_king_pos = if piece.kind == PieceKind::King { to } else { king_pos };
                    if !is_check_to(rules, &grid, new_king_pos) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// Grid is guaratneed to be returned intact.
fn is_bughouse_mate_to(
    rules: &ChessRules, grid: &mut Grid, king_pos: Coord, en_passant_target: Option<Coord>,
) -> bool {
    let force = king_force(grid, king_pos);
    if !is_chess_mate_to(rules, grid, king_pos, en_passant_target) {
        return false;
    }
    for pos in grid.shape().coords() {
        if grid[pos].is_none() {
            let grid = grid.scoped_set(
                pos,
                Some(PieceOnBoard::new(
                    PieceId::tmp(),
                    PieceKind::Queen,
                    PieceOrigin::Dropped,
                    force.into(),
                )),
            );
            if !is_check_to(rules, &grid, king_pos) {
                return false;
            }
        }
    }
    true
}

fn is_check_to(rules: &ChessRules, grid: &Grid, king_pos: Coord) -> bool {
    assert!(!rules.regicide());
    let force = king_force(grid, king_pos);
    for from in grid.shape().coords() {
        if let Some(piece) = grid[from] {
            if piece.force == force.opponent().into()
                && reachability(rules, grid, from, king_pos, true).ok()
            {
                return true;
            }
        }
    }
    false
}

// Returns the set of pieces that are attacking a given square.
fn attacker_set(
    rules: &ChessRules, grid: &Grid, pos: Coord, en_passant_target: Option<Coord>,
) -> HashSet<Coord> {
    let mut ret = HashSet::new();
    for from in grid.shape().coords() {
        if grid[from].is_some() {
            let capture = get_capture(grid, from, pos, en_passant_target);
            if reachability(rules, grid, from, pos, capture.is_some()).ok() {
                ret.insert(from);
            }
        }
    }
    ret
}

fn reachability(
    rules: &ChessRules, grid: &Grid, from: Coord, to: Coord, capturing: bool,
) -> Reachability {
    let capturing = if capturing { Capturing::Yes } else { Capturing::No };
    generic_reachability(rules, grid, from, to, capturing)
}

fn is_reachable_for_premove(rules: &ChessRules, grid: &Grid, from: Coord, to: Coord) -> bool {
    use Reachability::*;
    match generic_reachability(rules, grid, from, to, Capturing::Maybe) {
        Reachable | Blocked => true,
        Impossible => false,
    }
}

// Tests that the piece can move in such a way and that the path is free.
// Does not support castling.
fn generic_reachability(
    rules: &ChessRules, grid: &Grid, from: Coord, to: Coord, capturing: Capturing,
) -> Reachability {
    use Reachability::*;
    match generic_reachability_modulo_destination_square(grid, from, to, capturing) {
        Blocked => Blocked,
        Impossible => Impossible,
        Reachable => {
            if let Some(dst_piece) = grid[to] {
                let src_piece = grid[from].unwrap();
                if dst_piece.force == src_piece.force {
                    if combine_pieces(rules, PieceId::tmp(), src_piece, dst_piece).is_some() {
                        return Reachable;
                    } else {
                        return Blocked;
                    }
                } else if dst_piece.force == PieceForce::Neutral {
                    // Duck cannot be captured.
                    return Blocked;
                }
            }
            Reachable
        }
    }
}

fn generic_reachability_modulo_destination_square(
    grid: &Grid, from: Coord, to: Coord, capturing: Capturing,
) -> Reachability {
    use Reachability::*;
    if to == from {
        return Impossible;
    }
    let force;
    let piece_kind;
    match grid[from] {
        Some(piece) => {
            force = piece.force;
            piece_kind = piece.kind;
        }
        None => {
            return Impossible;
        }
    }

    let mut ret = Impossible;
    for &m in piece_kind.movements() {
        let r =
            reachability_by_movement_modulo_destination_square(grid, from, to, force, capturing, m);
        ret = combine_reachability(ret, r);
    }
    ret
}

fn reachability_by_movement_modulo_destination_square(
    grid: &Grid, from: Coord, to: Coord, force: PieceForce, capturing: Capturing,
    movement: PieceMovement,
) -> Reachability {
    use Reachability::*;
    match movement {
        PieceMovement::Leap { shift } => {
            if sort_two(tuple_abs(to - from)) == sort_two(shift) {
                Reachable
            } else {
                Impossible
            }
        }
        PieceMovement::Ride { shift, max_leaps } => {
            let d = to - from;
            let d_abs = tuple_abs(d);
            let mut shift_sorted = sort_two(shift);
            if d_abs.0 > d_abs.1 {
                mem::swap(&mut shift_sorted.0, &mut shift_sorted.1);
            }
            let Some(shift_directed) =
                apply_sign(shift_sorted.0, d.0).zip(apply_sign(shift_sorted.1, d.1))
            else {
                return Impossible;
            };
            let mut p = from + shift_directed;
            let mut blocked = false;
            let mut leaps: u8 = 1;
            while grid.contains_coord(p) {
                if p == to {
                    return if blocked { Blocked } else { Reachable };
                }
                if grid[p].is_some() {
                    blocked = true;
                }
                leaps += 1;
                if let Some(max_leaps) = max_leaps {
                    if leaps > max_leaps {
                        return Impossible;
                    }
                }
                p = p + shift_directed;
            }
            Impossible
        }
        PieceMovement::LikePawn => {
            let force = force.try_into().unwrap(); // unwrap ok: pawns cannot be neutral
            let (d_row, d_col) = to - from;
            let dir_forward = direction_forward(force);
            let src_row_subjective = SubjectiveRow::from_row(grid.shape(), from.row, force);
            let valid_capturing_move = d_col.abs() == 1 && d_row == dir_forward;
            let valid_non_capturing_move = d_col == 0
                && (d_row == dir_forward
                    || (src_row_subjective.to_one_based() <= 2 && d_row == dir_forward * 2));
            let is_path_free = || match d_row.abs() {
                1 => true,
                2 => grid.get(from + (dir_forward, 0)).is_free(),
                _ => panic!("Unexpected pawn move distance: {d_row}"),
            };
            let capturing_reachability = match (valid_capturing_move, capturing) {
                (false, _) => Impossible,
                (true, Capturing::No) => Blocked,
                (true, Capturing::Yes | Capturing::Maybe) => Reachable,
            };
            let non_capturing_reachability = match (valid_non_capturing_move, capturing) {
                (false, _) => Impossible,
                (true, Capturing::Yes) => Blocked,
                (true, Capturing::No | Capturing::Maybe) => {
                    if is_path_free() {
                        Reachable
                    } else {
                        Blocked
                    }
                }
            };
            combine_reachability(capturing_reachability, non_capturing_reachability)
        }
        PieceMovement::FreeSquare => {
            if grid[to].is_some() {
                Blocked
            } else {
                Reachable
            }
        }
    }
}

fn combine_reachability(a: Reachability, b: Reachability) -> Reachability {
    use Reachability::*;
    match (a, b) {
        (Reachable, _) | (_, Reachable) => Reachable,
        (Blocked, _) | (_, Blocked) => Blocked,
        (Impossible, Impossible) => Impossible,
    }
}

fn initial_castling_rights(starting_position: &EffectiveStartingPosition) -> CastlingRights {
    let row = starting_piece_row(starting_position);
    let king_pos = row.iter().position(|&p| p == PieceKind::King).unwrap();
    let king_col = Col::from_zero_based(king_pos.try_into().unwrap());
    let mut rights = enum_map! { _ => None };
    for (col, &piece) in row.iter().enumerate() {
        let col = Col::from_zero_based(col.try_into().unwrap());
        if piece == PieceKind::Rook {
            use CastleDirection::*;
            let dir = if col < king_col { ASide } else { HSide };
            assert!(rights[dir].is_none());
            rights[dir] = Some(col);
        }
    }
    rights
}

fn remove_castling_right(castling_rights: &mut CastlingRights, col: Col) {
    for (_, col_rights) in castling_rights.iter_mut() {
        if *col_rights == Some(col) {
            *col_rights = None;
        }
    }
}


// Whether the piece is going to capture. Used by reachability tests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Capturing {
    Yes,   // used by TurnMode::Normal
    No,    // used by TurnMode::Normal
    Maybe, // used by TurnMode::Preturn
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reachability {
    Reachable,
    Blocked,
    Impossible,
}

#[derive(Clone, Copy, Debug)]
pub struct CastlingRelocations {
    pub king: (Coord, Coord),
    pub rook: (Coord, Coord),
}

#[derive(Clone, Debug)]
struct TurnOutcome {
    new_grid: Grid,
    facts: TurnFacts,
}

#[derive(Clone, Debug)]
pub struct TurnFacts {
    pub castling_relocations: Option<CastlingRelocations>,
    pub next_piece_id: PieceId,
    pub reserve_reduction: Option<PieceKind>,
    pub captures: Vec<Capture>,
    pub steals: Vec<Steal>,
}

#[derive(Clone, Copy, Debug)]
pub struct Capture {
    pub from: Option<Coord>,
    pub piece_kind: PieceKind,
    pub force: PieceForce,
}

#[derive(Clone, Copy, Debug)]
pub struct Steal {
    pub piece_id: PieceId,
    pub piece_kind: PieceKind,
    pub force: PieceForce,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PromotionTarget {
    Upgrade(PieceKind),
    Discard,
    // `Coord` would be sufficient in a local game, but `PieceId` is more robust when playing online
    // and pieces on the other boards move quickly. At implemented, we don't have the same benefit
    // with algebraic input. If this becomes a problem, one way to deal with it would be convert
    // `TurnInput` to contain a parsed turn representation with additional flags that convey all
    // additional information (e.g. `must_capture == true` if algebraic input contained "x").
    Steal((PieceKind, PieceOrigin, PieceId)),
}

// Note. Generally speaking, it's impossible to detect castling based on king movement in Chess960.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Turn {
    Move(TurnMove),
    Drop(TurnDrop),
    Castle(CastleDirection),
    // Use a special turn kind for duck relocations instead of `Move`/`Drop`, because:
    //   - Is enables duck preturns. With a regular `Move` duck preturns would fail because
    //     the duck is no longer in the source location;
    //   - It gives more control over the algebraic notation (we still don't get the proper
    //     notation for duck chess, but at least it's something).
    PlaceDuck(Coord),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct TurnMove {
    pub from: Coord,
    pub to: Coord,
    pub promote_to: Option<PromotionTarget>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct TurnDrop {
    pub piece_kind: PieceKind,
    pub to: Coord,
}

// Turn, as entered by user.
//
// Since each turn can be interpreted slightly differently depending on input method (details
// below), all pre-turns should be stored as `TurnInput` until they are ready to be executed
// as in-order turns.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TurnInput {
    // Explicit turn can be used when a turn has already been parsed earlier, e.g. for replays.
    Explicit(Turn),

    // Turn made via mouse or touch drag&drop. The `Turn` object inside is preliminary, it can
    //   be altered in order to allow reinterpreting king movement as castling.
    //
    // Castling rules for drag-and-drop interfaces:
    //   (a) drag the king at least two squares in the rook direction, or
    //   (b) onto a rook.
    // In case (a) castling in unambiguous and DragDrop will contain Turn::Castle.
    // In case (b) DragDrop will contain Turn::Move that resolves to a castle if the rook is
    //   still there or to a move if the rook was captured.
    //
    // The difference is only meaningful for pre-turns. Options (a) and (b) are synonyms for
    //   in-order turns.
    // Note. In some starting positions in Fischer random option (b) is the only way to castle.
    DragDrop(Turn),

    // Turn entered as algebraic notation.
    //
    // Note. Only by storing the text as is we can preserve some useful pieces of metainformation
    //   for preturns, e.g. to make sure that "xd5" fails if it's not capturing.
    Algebraic(String),
}

// Turn annotated with additional information for highlights and log beautification.
#[derive(Clone, Debug)]
pub struct TurnExpanded {
    pub turn: Turn,
    pub algebraic: AlgebraicTurn,
    pub relocation: Option<(Coord, Coord)>,
    pub relocation_extra: Option<(Coord, Coord)>,
    pub drop: Option<Coord>,
    pub captures: Vec<Capture>,
    pub steals: Vec<Steal>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnMode {
    // Regular in-order turn.
    Normal,

    // Out-of-order turn scheduled for execution. This is normally called "premove",
    // but we reserve "move" for a turn that takes one piece from the board and moves
    // it to another place on the board.
    //
    // A single preturn puts the game into an irrecoverably broken stake and should
    // never be executed on the main copy of the game.
    //
    // Assumptions for preturn:
    //   - Opponent pieces may have been removed, relocated or added.
    //   - Current player pieces may have been removed, but NOT relocated or added.
    // Validity test for a preturn is a strict as possible given these assuptions,
    // but not stricter.
    //
    // TODO: Classify TurnError-s into those that are ok for a pre-turn and those that
    // and not; test that a preturn is rejected iff the error is irrecoverable.
    Preturn,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VictoryReason {
    Checkmate,
    Flag,
    Resignation,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DrawReason {
    SimultaneousFlag, // for bughouse
    ThreefoldRepetition,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChessGameStatus {
    Active,
    Victory(Force, VictoryReason),
    Draw(DrawReason),
}

// Improvement potential: Consistent naming. Either always describe what went wrong, or always
// describe what should have happened. The first one is used more often, but second one is also
// used, e.g. `...Requires...` or `Must...`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnError {
    NotPlayer,
    InvalidNotation,
    AmbiguousNotation,
    CaptureNotationRequiresCapture,
    PieceMissing,
    WrongTurnOrder,
    PreturnLimitReached,
    ImpossibleTrajectory,
    PathBlocked,
    UnprotectedKing,
    CastlingPieceHasMoved,
    CannotCastleDroppedKing,
    BadPromotionType,
    MustPromoteHere,
    CannotPromoteHere,
    InvalidUpgradePromotionTarget,
    InvalidStealPromotionTarget,
    DropRequiresBughouse,
    DropPieceMissing,
    InvalidPawnDropRank,
    DropBlocked,
    DropAggression,
    StealTargetMissing,
    StealTargetInvalid,
    ExposingKingByStealing,
    ExposingPartnerKingByStealing,
    NotDuckChess,
    DuckPlacementIsSpecialTurnKind,
    MustMovePieceBeforeDuck,
    MustPlaceDuck,
    MustChangeDuckPosition,
    MustDropKingIfPossible,
    NoGameInProgress,
    GameOver,
    WaybackIsActive,
}

pub type Reserve = EnumMap<PieceKind, u8>;

pub type CastlingRights = EnumMap<CastleDirection, Option<Col>>;

// In classic chess, positions are compared for threefold repetition using FIDE rules:
//
//   Two positions are by definition "the same" if the same types of pieces occupy the same
//   squares, the same player has the move, the remaining castling rights are the same and
//   the possibility to capture en passant is the same.
//
// For bughouse the total number of drops is included in addition. This effectively resets
// the counter every time a piece is dropped. Note that it could potentially lead to an
// infinite exchange loop involving both boards. But, given how unlikely this outcome is,
// it seems better than not having this rule.
//
// Improvement potential. Add rules to detect infinite loops involving both boards.
// Improvement potential. If this becomes a performance bottleneck, we could remove
// `total_drops` and instead clear the position set after every drop (as well as every
// capture, castling and pawn move).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct PositionForRepetitionDraw {
    grid: GridForRepetitionDraw,
    active_force: Force,
    castling_rights: EnumMap<Force, CastlingRights>,
    en_passant_target: Option<Coord>,
    total_drops: u32,
}


impl Reachability {
    pub fn ok(self) -> bool { self == Reachability::Reachable }
}

// Improvement potential: Don't store players here since they don't affect game process.
#[derive(Clone, Debug)]
pub struct Board {
    rules: Rc<Rules>,
    player_names: EnumMap<Force, String>,
    status: ChessGameStatus,
    grid: Grid,
    next_piece_id: PieceId,
    // Tracks castling availability based on which pieces have moved. Castling is
    // allowed when the rook stands in the first row at specified columns. If the
    // king has moved then the list is empty. Not affected by temporary limitations
    // (e.g. the king being checked).
    castling_rights: EnumMap<Force, CastlingRights>,
    en_passant_target: Option<Coord>,
    reserves: EnumMap<Force, Reserve>,
    total_drops: u32, // total number of drops from both sides
    position_count: HashMap<PositionForRepetitionDraw, u32>,
    clock: Clock,
    active_force: Force,
    is_duck_turn: EnumMap<Force, bool>, // track separately per force to allow preturns
}

impl Board {
    pub fn new(
        rules: Rc<Rules>, players: EnumMap<Force, String>,
        starting_position: &EffectiveStartingPosition,
    ) -> Board {
        let board_shape = rules.chess_rules.board_shape();
        let mut next_piece_id = PieceId::new();
        let grid = generate_starting_grid(board_shape, starting_position, &mut next_piece_id);
        let each_castling_rights = initial_castling_rights(starting_position);
        let castling_rights = enum_map! { _ => each_castling_rights };
        let mut reserves = enum_map! { _ => enum_map!{ _ => 0 } };
        if rules.chess_rules.duck_chess {
            reserves[Force::White][PieceKind::Duck] += 1;
        }
        let setup = BoardSetup {
            grid,
            next_piece_id,
            castling_rights,
            en_passant_target: None,
            reserves,
            active_force: Force::White,
        };
        Self::new_from_setup(rules, players, setup)
    }

    pub fn new_from_setup(
        rules: Rc<Rules>, players: EnumMap<Force, String>, setup: BoardSetup,
    ) -> Board {
        let time_control = rules.chess_rules.time_control.clone();
        let mut board = Board {
            rules,
            player_names: players,
            status: ChessGameStatus::Active,
            grid: setup.grid,
            next_piece_id: setup.next_piece_id,
            castling_rights: setup.castling_rights,
            en_passant_target: setup.en_passant_target,
            reserves: setup.reserves,
            total_drops: 0,
            position_count: HashMap::new(),
            clock: Clock::new(time_control),
            active_force: setup.active_force,
            is_duck_turn: enum_map! { _ => false },
        };
        board.log_position_for_repetition_draw();
        board
    }

    pub fn clone_for_wayback(&self) -> Self {
        Board {
            // Strip away large structures that are not needed for rendering:
            position_count: HashMap::new(),

            // Copy everything else as is:
            rules: self.rules.clone(),
            player_names: self.player_names.clone(),
            grid: self.grid.clone(),
            clock: self.clock.clone(),
            ..*self
        }
    }

    pub fn rules(&self) -> &Rules { &self.rules }
    pub fn match_rules(&self) -> &MatchRules { &self.rules.match_rules }
    pub fn chess_rules(&self) -> &ChessRules { &self.rules.chess_rules }
    pub fn bughouse_rules(&self) -> Option<&BughouseRules> {
        self.rules.chess_rules.bughouse_rules.as_ref()
    }
    pub fn player_name(&self, force: Force) -> &str { &self.player_names[force] }
    pub fn player_names(&self) -> &EnumMap<Force, String> { &self.player_names }
    pub fn status(&self) -> ChessGameStatus { self.status }
    pub fn shape(&self) -> BoardShape { self.grid.shape() }
    pub fn grid(&self) -> &Grid { &self.grid }
    pub fn grid_mut(&mut self) -> &mut Grid { &mut self.grid }
    pub fn castling_rights(&self) -> &EnumMap<Force, CastlingRights> { &self.castling_rights }
    pub fn en_passant_target(&self) -> Option<Coord> { self.en_passant_target }
    pub fn reserve(&self, force: Force) -> &Reserve { &self.reserves[force] }
    pub fn reserve_mut(&mut self, force: Force) -> &mut Reserve { &mut self.reserves[force] }
    pub fn reserves(&self) -> &EnumMap<Force, Reserve> { &self.reserves }
    pub fn clock(&self) -> &Clock { &self.clock }
    pub fn clock_mut(&mut self) -> &mut Clock { &mut self.clock }
    pub fn active_force(&self) -> Force { self.active_force }
    pub fn is_duck_turn(&self, force: Force) -> bool { self.is_duck_turn[force] }

    pub fn is_bughouse(&self) -> bool { self.bughouse_rules().is_some() }
    pub fn turn_owner(&self, mode: TurnMode) -> Force {
        match mode {
            TurnMode::Normal => self.active_force,
            TurnMode::Preturn => self.active_force.opponent(),
        }
    }

    pub fn start_clock(&mut self, now: GameInstant) {
        if !self.clock.is_active() {
            self.clock.new_turn(self.active_force, now);
        }
    }
    pub fn flag_defeat_moment(&self, now: GameInstant) -> Option<GameInstant> {
        assert_eq!(self.status, ChessGameStatus::Active);
        self.clock
            .time_excess(self.active_force, now)
            .map(|excess| now.checked_sub(excess).unwrap())
    }
    pub fn test_flag(&mut self, now: GameInstant) {
        assert_eq!(self.status, ChessGameStatus::Active);
        if self.clock.time_left(self.active_force, now).is_zero() {
            self.status =
                ChessGameStatus::Victory(self.active_force.opponent(), VictoryReason::Flag);
        }
    }

    // Whether a given side can move a given piece, now or later.
    pub fn can_potentially_move_piece(&self, envoy_force: Force, piece_force: PieceForce) -> bool {
        match Force::try_from(piece_force) {
            Err(()) => true,
            Ok(force) => force == envoy_force,
        }
    }

    // Does not test flag. Will not update game status if a player has zero time left.
    pub fn try_turn(
        &mut self, turn: Turn, mode: TurnMode, now: GameInstant,
    ) -> Result<TurnFacts, TurnError> {
        // Turn application is split into two phases:
        //   - First, check turn validity and determine the outcome (does not change
        //     game state, can fail if the turn is invalid).
        //   - Second, apply the outcome (changes game state, cannot fail).
        let TurnOutcome { new_grid, facts } = self.turn_outcome(turn, mode)?;
        self.apply_turn(turn, mode, new_grid, &facts, now);
        Ok(facts)
    }

    pub fn parse_turn_input(
        &self, turn_input: &TurnInput, mode: TurnMode, other_board: Option<&Board>,
    ) -> Result<Turn, TurnError> {
        Ok(match turn_input {
            TurnInput::Explicit(turn) => *turn,
            TurnInput::DragDrop(turn) => self.parse_drag_drop_turn(*turn, mode)?,
            TurnInput::Algebraic(notation) => {
                let notation_parsed =
                    AlgebraicTurn::parse(notation).ok_or(TurnError::InvalidNotation)?;
                self.algebraic_to_turn(&notation_parsed, mode, other_board)?
            }
        })
    }

    pub fn is_legal_move_destination(&self, from: Coord, to: Coord, mode: TurnMode) -> bool {
        match mode {
            TurnMode::Normal => {
                let capture = get_capture(&self.grid, from, to, self.en_passant_target);
                reachability(self.chess_rules(), &self.grid, from, to, capture.is_some()).ok()
            }
            TurnMode::Preturn => is_reachable_for_premove(self.chess_rules(), &self.grid, from, to),
        }
    }

    // Generates legal moves and castlings (if King) for a piece in a given square.
    // Check and mate are not taken into account.
    pub fn legal_turn_destinations(&self, from: Coord) -> Vec<Coord> {
        // TODO: What about preturns? Possibilities:
        //   - Treat as a normal turn (this happens now),
        //   - Include all possibilities,
        //   - Return two separate lists: normal turn moves + preturn moves.
        let mut ret =
            legal_move_destinations(self.chess_rules(), &self.grid, from, self.en_passant_target);
        ret.extend(legal_castling_destinations(&self.grid, from, &self.castling_rights));
        ret
    }

    pub fn stealing_result(&self, pos: Coord, thief: Force) -> Result<(), TurnError> {
        let partner = thief.opponent();
        let Some(piece) = self.grid[pos] else {
            return Err(TurnError::StealTargetMissing);
        };
        if !piece.kind.can_be_steal_promotion_target() {
            return Err(TurnError::StealTargetInvalid);
        }
        if piece.force != partner.opponent().into() {
            return Err(TurnError::StealTargetInvalid);
        }
        if !self.chess_rules().regicide() {
            for king_owner in [partner, partner.opponent()] {
                // Technically we don't need the `clone` because of `scoped_set`. However removing
                // the `clone` would complicate the API (we'll have to use `&mut self`), and steal
                // promtions are rare.
                let mut grid = self.grid.clone();
                let king_pos = find_king(&grid, king_owner).unwrap();
                let attackers_before =
                    attacker_set(self.chess_rules(), &grid, king_pos, self.en_passant_target);
                let grid = grid.scoped_set(pos, None);
                let attackers_after =
                    attacker_set(self.chess_rules(), &grid, king_pos, self.en_passant_target);
                if !attackers_after.is_subset(&attackers_before) {
                    if king_owner == partner {
                        return Err(TurnError::ExposingPartnerKingByStealing);
                    } else {
                        return Err(TurnError::ExposingKingByStealing);
                    }
                }
            }
        }
        Ok(())
    }

    // For fog-of-war variant.
    pub fn fog_free_area(&self, force: Force) -> HashSet<Coord> {
        let mut ret = HashSet::new();
        for from in self.shape().coords() {
            if let Some(piece) = self.grid[from] {
                if piece.force == force.into() {
                    ret.insert(from);
                    ret.extend(visibility_from(
                        self.chess_rules(),
                        &self.grid,
                        from,
                        self.en_passant_target,
                    ))
                } else if piece.force == PieceForce::Neutral {
                    ret.insert(from);
                }
            }
        }
        ret
    }

    fn log_position_for_repetition_draw(&mut self) {
        let position_for_repetition_draw = PositionForRepetitionDraw {
            grid: self
                .grid
                .map(|piece| PieceForRepetitionDraw { kind: piece.kind, force: piece.force }),
            active_force: self.active_force,
            castling_rights: self.castling_rights,
            en_passant_target: self.en_passant_target,
            total_drops: self.total_drops,
        };
        let num_repetition = self.position_count.entry(position_for_repetition_draw).or_insert(0);
        *num_repetition += 1;
        if *num_repetition >= 3 {
            self.status = ChessGameStatus::Draw(DrawReason::ThreefoldRepetition);
        }
    }

    fn update_turn_stage_and_active_force(&mut self, mode: TurnMode) {
        let force = self.turn_owner(mode);
        let next_active_force = match mode {
            TurnMode::Normal => self.active_force.opponent(),
            TurnMode::Preturn => self.active_force,
        };
        if self.chess_rules().duck_chess {
            if self.is_duck_turn[force] {
                self.is_duck_turn[force] = false;
                self.active_force = next_active_force;
            } else {
                self.is_duck_turn[force] = true;
            }
        } else {
            self.active_force = next_active_force;
        }
    }

    fn apply_turn(
        &mut self, turn: Turn, mode: TurnMode, new_grid: Grid, facts: &TurnFacts, now: GameInstant,
    ) {
        self.next_piece_id = facts.next_piece_id;
        let force = self.turn_owner(mode);
        assert_eq!(self.is_duck_turn[force], matches!(turn, Turn::PlaceDuck(_)));
        match &turn {
            Turn::Move(mv) => {
                let first_row = SubjectiveRow::first().to_row(self.shape(), force);
                let piece = &mut self.grid[mv.from].unwrap();
                if piece.kind == PieceKind::King {
                    self.castling_rights[force].clear();
                } else if piece.kind == PieceKind::Rook && mv.from.row == first_row {
                    remove_castling_right(&mut self.castling_rights[force], mv.from.col);
                }
                for capture in facts.captures.iter() {
                    if let Ok(f) = capture.force.try_into() {
                        let first_row = SubjectiveRow::first().to_row(self.shape(), f);
                        if mv.to.row == first_row && capture.piece_kind == PieceKind::Rook {
                            remove_castling_right(&mut self.castling_rights[f], mv.to.col);
                        }
                    }
                }
            }
            Turn::Drop(_) => {
                self.total_drops += 1;
            }
            Turn::Castle(_) => {
                self.castling_rights[force].clear();
            }
            Turn::PlaceDuck(_) => {}
        }
        self.grid = new_grid;
        if let Some(piece_kind) = facts.reserve_reduction {
            let reserve_left = &mut self.reserves[force][piece_kind];
            if *reserve_left > 0 {
                *reserve_left -= 1;
            } else {
                if mode == TurnMode::Normal {
                    panic!("Must have verified reserve earlier");
                }
            }
        }

        match mode {
            TurnMode::Normal => {
                if !matches!(turn, Turn::PlaceDuck(_)) {
                    self.en_passant_target = get_en_passant_target(&self.grid, turn);
                }
                if !self.chess_rules().regicide() {
                    let opponent_king_pos = find_king(&self.grid, force.opponent()).unwrap();
                    if self.is_bughouse() {
                        if is_bughouse_mate_to(
                            &self.rules.chess_rules,
                            &mut self.grid,
                            opponent_king_pos,
                            self.en_passant_target,
                        ) {
                            self.status = ChessGameStatus::Victory(force, VictoryReason::Checkmate);
                        }
                    } else {
                        if is_chess_mate_to(
                            &self.rules.chess_rules,
                            &mut self.grid,
                            opponent_king_pos,
                            self.en_passant_target,
                        ) {
                            self.status = ChessGameStatus::Victory(force, VictoryReason::Checkmate);
                        }
                    }
                } else {
                    // This assumes opposite kings cannot be capture simultaneously.
                    for capture in facts.captures.iter() {
                        if capture.piece_kind == PieceKind::King {
                            if self.bughouse_rules().map_or(false, |r| r.koedem) {
                                // Cannot check victory condition here, need to see both boards.
                            } else {
                                // Unwrap ok: King cannot be neutral.
                                let loser: Force = capture.force.try_into().unwrap();
                                self.status = ChessGameStatus::Victory(
                                    loser.opponent(),
                                    VictoryReason::Checkmate,
                                );
                            }
                        }
                    }
                }
                self.update_turn_stage_and_active_force(mode);
                self.clock.new_turn(self.active_force, now);
                self.log_position_for_repetition_draw();
            }
            TurnMode::Preturn => {
                self.en_passant_target = None;
                self.update_turn_stage_and_active_force(mode);
            }
        }
    }

    fn turn_outcome(&self, turn: Turn, mode: TurnMode) -> Result<TurnOutcome, TurnError> {
        let mut outcome = self.turn_outcome_no_check_test(turn, mode)?;
        match mode {
            TurnMode::Normal => self.verify_check_and_drop_aggression(turn, mode, &mut outcome)?,
            TurnMode::Preturn => {}
        }
        Ok(outcome)
    }

    // `outcome` is guaratneed to be returned intact.
    fn verify_check_and_drop_aggression(
        &self, turn: Turn, mode: TurnMode, outcome: &mut TurnOutcome,
    ) -> Result<(), TurnError> {
        if self.chess_rules().regicide() {
            return Ok(());
        }
        let new_grid = &mut outcome.new_grid;
        let force = self.turn_owner(mode);
        let king_pos = find_king(new_grid, force).unwrap();
        let opponent_king_pos = find_king(new_grid, force.opponent()).unwrap();
        if is_check_to(self.chess_rules(), new_grid, king_pos) {
            return Err(TurnError::UnprotectedKing);
        }
        if let Turn::Drop(_) = turn {
            let bughouse_rules = self.bughouse_rules().unwrap(); // unwrap ok: tested earlier
            let drop_legal = match bughouse_rules.drop_aggression {
                DropAggression::NoCheck => {
                    !is_check_to(self.chess_rules(), new_grid, opponent_king_pos)
                }
                DropAggression::NoChessMate => !is_chess_mate_to(
                    self.chess_rules(),
                    new_grid,
                    opponent_king_pos,
                    self.en_passant_target,
                ),
                DropAggression::NoBughouseMate => !is_bughouse_mate_to(
                    self.chess_rules(),
                    new_grid,
                    opponent_king_pos,
                    self.en_passant_target,
                ),
                DropAggression::MateAllowed => true,
            };
            if !drop_legal {
                return Err(TurnError::DropAggression);
            }
        }
        Ok(())
    }

    fn turn_outcome_no_check_test(
        &self, turn: Turn, mode: TurnMode,
    ) -> Result<TurnOutcome, TurnError> {
        if self.status != ChessGameStatus::Active {
            return Err(TurnError::GameOver);
        }
        let force = self.turn_owner(mode);
        if self.bughouse_rules().map_or(false, |r| r.koedem)
            && self.reserve(force)[PieceKind::King] > 0
        {
            let ok = match turn {
                Turn::Move(_) | Turn::Castle(_) => false,
                Turn::Drop(TurnDrop { piece_kind, .. }) => piece_kind == PieceKind::King,
                Turn::PlaceDuck(_) => true,
            };
            if !ok {
                return Err(TurnError::MustDropKingIfPossible);
            }
        }
        let mut new_grid = self.grid.clone();
        let mut castling_relocations = None;
        let mut next_piece_id = self.next_piece_id;
        let mut reserve_reduction = None;
        let mut captures = vec![];
        let mut steals = vec![];
        match turn {
            Turn::Move(mv) => {
                let piece = new_grid[mv.from].ok_or(TurnError::PieceMissing)?;
                if piece.kind == PieceKind::Duck {
                    return Err(TurnError::DuckPlacementIsSpecialTurnKind);
                }
                if self.is_duck_turn[force] {
                    return Err(TurnError::MustPlaceDuck);
                }
                if piece.force != force.into() {
                    return Err(TurnError::WrongTurnOrder);
                }
                let mut capture_pos_or = None;
                match mode {
                    TurnMode::Normal => {
                        use Reachability::*;
                        capture_pos_or =
                            get_capture(&new_grid, mv.from, mv.to, self.en_passant_target);
                        match reachability(
                            self.chess_rules(),
                            &new_grid,
                            mv.from,
                            mv.to,
                            capture_pos_or.is_some(),
                        ) {
                            Reachable => {}
                            Blocked => return Err(TurnError::PathBlocked),
                            Impossible => return Err(TurnError::ImpossibleTrajectory),
                        }
                    }
                    TurnMode::Preturn => {
                        if !is_reachable_for_premove(self.chess_rules(), &new_grid, mv.from, mv.to)
                        {
                            return Err(TurnError::ImpossibleTrajectory);
                        }
                    }
                }
                new_grid[mv.from] = None;
                if let Some(capture_pos) = capture_pos_or {
                    let captured_piece = new_grid[capture_pos].unwrap();
                    let captured_piece_kinds = match captured_piece.origin {
                        PieceOrigin::Innate | PieceOrigin::Dropped => vec![captured_piece.kind],
                        PieceOrigin::Promoted => vec![PieceKind::Pawn],
                        PieceOrigin::Combined((p1, p2)) => vec![p1, p2],
                    };
                    captures.extend(captured_piece_kinds.into_iter().map(|piece_kind| Capture {
                        from: Some(capture_pos),
                        piece_kind,
                        force: captured_piece.force,
                    }));
                    new_grid[capture_pos] = None;
                }
                // Verify that requested promotion does not violate promotion rules.
                match (mv.promote_to, self.chess_rules().promotion()) {
                    // No promotion - no problem.
                    (None, _) => {}
                    // Promotion rules match.
                    (Some(PromotionTarget::Upgrade(..)), Promotion::Upgrade)
                    | (Some(PromotionTarget::Discard), Promotion::Discard)
                    | (Some(PromotionTarget::Steal(..)), Promotion::Steal) => {}
                    // Promotion type doesn't match game rules. The user shouldn't be able to
                    // achieve this via drag&drop, so it means eather bad algebraic notation, or an
                    // internal error.
                    (Some(_), _) => return Err(TurnError::BadPromotionType),
                };
                if should_promote(self.shape(), force, piece.kind, mv.to) {
                    let Some(promote_to) = mv.promote_to else {
                        return Err(TurnError::MustPromoteHere);
                    };
                    match promote_to {
                        PromotionTarget::Upgrade(promo_piece_kind) => {
                            if !promo_piece_kind.can_be_upgrade_promotion_target() {
                                return Err(TurnError::InvalidUpgradePromotionTarget);
                            }
                            new_grid[mv.to] = Some(PieceOnBoard::new(
                                next_piece_id.inc(),
                                promo_piece_kind,
                                PieceOrigin::Promoted,
                                piece.force,
                            ));
                        }
                        PromotionTarget::Discard => {
                            // Give the pawn to the diagonal opponent.
                            captures.push(Capture {
                                from: None, // don't highlight anything: it's not really a capture
                                piece_kind: piece.kind,
                                force: piece.force,
                            });
                        }
                        PromotionTarget::Steal((
                            promo_piece_kind,
                            promo_piece_origin,
                            promo_piece_id,
                        )) => {
                            if !promo_piece_kind.can_be_steal_promotion_target() {
                                return Err(TurnError::InvalidStealPromotionTarget);
                            }
                            // Give the pawn to the diagonal opponent in exchange for the stolen piece.
                            captures.push(Capture {
                                from: None, // don't highlight anything: it's not really a capture
                                piece_kind: piece.kind,
                                force: piece.force,
                            });
                            steals.push(Steal {
                                piece_id: promo_piece_id,
                                piece_kind: promo_piece_kind,
                                force: piece.force,
                            });
                            let origin = match promo_piece_origin {
                                // Not `Promoted`: piece shouldn't convert to pawn on capture.
                                PieceOrigin::Innate => PieceOrigin::Dropped,
                                PieceOrigin::Dropped => PieceOrigin::Dropped,
                                // Shouldn't happen: this would imply promotion strategies can be
                                // mixed, which they can't. Assuming promotion mixing exists, we
                                // should keep the origin for "mass preservation". That is, not
                                // transmuting pawns into pieces permanently.
                                PieceOrigin::Promoted => PieceOrigin::Promoted,
                                // Preserve composition information, so that the piece falls apart
                                // properly later when captured.
                                PieceOrigin::Combined(_) => promo_piece_origin,
                            };
                            new_grid[mv.to] = Some(PieceOnBoard::new(
                                next_piece_id.inc(),
                                promo_piece_kind,
                                origin,
                                piece.force,
                            ));
                        }
                    }
                } else {
                    if mv.promote_to.is_some() {
                        return Err(TurnError::CannotPromoteHere);
                    } else if let Some(dst_piece) = new_grid[mv.to] {
                        if let Some(combined_piece) = combine_pieces(
                            self.chess_rules(),
                            next_piece_id.inc(),
                            piece,
                            dst_piece,
                        ) {
                            new_grid[mv.to] = Some(combined_piece);
                        } else {
                            assert_eq!(mode, TurnMode::Preturn);
                            new_grid[mv.to] = Some(piece);
                        }
                    } else {
                        new_grid[mv.to] = Some(piece);
                    }
                }
            }
            Turn::Drop(drop) => {
                let bughouse_rules =
                    self.bughouse_rules().ok_or(TurnError::DropRequiresBughouse)?;
                if drop.piece_kind == PieceKind::Duck {
                    return Err(TurnError::DuckPlacementIsSpecialTurnKind);
                }
                if self.is_duck_turn[force] {
                    return Err(TurnError::MustPlaceDuck);
                }
                let to_subjective_row = SubjectiveRow::from_row(self.shape(), drop.to.row, force);
                if drop.piece_kind == PieceKind::Pawn
                    && (to_subjective_row < bughouse_rules.min_pawn_drop_rank
                        || to_subjective_row > bughouse_rules.max_pawn_drop_rank)
                {
                    return Err(TurnError::InvalidPawnDropRank);
                }
                // Improvement potential: Allow pre-turns dropping missing pieces.
                if self.reserves[force][drop.piece_kind] < 1 {
                    return Err(TurnError::DropPieceMissing);
                }
                let piece_force = drop.piece_kind.reserve_piece_force(force);
                let mut new_piece = PieceOnBoard::new(
                    next_piece_id.inc(),
                    drop.piece_kind,
                    PieceOrigin::Dropped,
                    piece_force,
                );
                if let Some(dst_piece) = new_grid[drop.to] {
                    if let Some(combined_piece) = combine_pieces(
                        self.chess_rules(),
                        next_piece_id.inc(),
                        new_piece,
                        dst_piece,
                    ) {
                        new_piece = combined_piece;
                    } else {
                        match mode {
                            TurnMode::Normal => {
                                return Err(TurnError::DropBlocked);
                            }
                            TurnMode::Preturn => {}
                        }
                    }
                }
                new_grid[drop.to] = Some(new_piece);
                reserve_reduction = Some(drop.piece_kind);
            }
            Turn::Castle(dir) => {
                // TODO: More castling tests. Include cases:
                //   - Castle successful.
                //   - Cannot castle when king has moved.
                //   - Cannot castle when rook has moved.
                //   - Cannot castle when there are pieces in between.
                //   - King cannot starts in a checked square.
                //   - King cannot pass through a checked square.
                //   - King cannot ends up in a checked square.
                //   - Cannot castle if rook was captured and another one was
                //     dropped on its place.
                //   - [Chess960] Castle blocked by a piece at the destination,
                //      which is outside of kind and rook initial positions.
                //   - [Chess960] Castle when both rooks are on the same side,
                //      both when it's possible (the other rook is further away)
                //      and impossible (the other rook is in the way).

                if self.is_duck_turn[force] {
                    return Err(TurnError::MustPlaceDuck);
                }
                let row = SubjectiveRow::first().to_row(self.shape(), force);
                // Can only castle the original king in koedem.
                let original_king_pos = find_piece(&new_grid, |p| {
                    p.kind == PieceKind::King
                        && p.force == force.into()
                        && p.origin == PieceOrigin::Innate
                });
                // King can be missing in case of pre-turns.
                let king_from = original_king_pos.ok_or(TurnError::CastlingPieceHasMoved)?;
                if king_from.row != row {
                    return Err(TurnError::CastlingPieceHasMoved);
                }
                let king = new_grid[king_from].take();

                let rook_col =
                    self.castling_rights[force][dir].ok_or(TurnError::CastlingPieceHasMoved)?;
                let rook_from = Coord::new(row, rook_col);
                let rook = new_grid[rook_from].take();
                assert!(matches!(rook, Some(PieceOnBoard { kind: PieceKind::Rook, .. })));

                let king_to;
                let rook_to;
                match dir {
                    CastleDirection::ASide => {
                        king_to = Coord::new(row, Col::C);
                        rook_to = Coord::new(row, Col::D);
                    }
                    CastleDirection::HSide => {
                        king_to = Coord::new(row, Col::G);
                        rook_to = Coord::new(row, Col::F);
                    }
                };

                match mode {
                    TurnMode::Normal => {
                        let cols = [king_from.col, king_to.col, rook_from.col, rook_to.col];
                        for col in col_range_inclusive(iter_minmax(cols.into_iter()).unwrap()) {
                            if new_grid[Coord::new(row, col)].is_some() {
                                return Err(TurnError::PathBlocked);
                            }
                        }

                        let cols = [king_from.col, king_to.col];
                        for col in col_range_inclusive(iter_minmax(cols.into_iter()).unwrap()) {
                            let pos = Coord::new(row, col);
                            let new_grid = new_grid.scoped_set(pos, king);
                            if !self.chess_rules().regicide()
                                && is_check_to(self.chess_rules(), &new_grid, pos)
                            {
                                return Err(TurnError::UnprotectedKing);
                            }
                        }
                    }
                    TurnMode::Preturn => {}
                }

                new_grid[king_to] = king;
                new_grid[rook_to] = rook;
                castling_relocations = Some(CastlingRelocations {
                    king: (king_from, king_to),
                    rook: (rook_from, rook_to),
                });
            }
            Turn::PlaceDuck(to) => {
                if !self.chess_rules().duck_chess {
                    return Err(TurnError::NotDuckChess);
                }
                if !self.is_duck_turn[force] {
                    return Err(TurnError::MustMovePieceBeforeDuck);
                }
                let from = find_piece(&new_grid, |p| p.kind == PieceKind::Duck);
                let duck = if let Some(from) = from {
                    if to == from && mode == TurnMode::Normal {
                        return Err(TurnError::MustChangeDuckPosition);
                    }
                    new_grid[from].take().unwrap()
                } else {
                    if self.reserves[force][PieceKind::Duck] == 0 && mode == TurnMode::Normal {
                        // This shouldn't really happen.
                        return Err(TurnError::DropPieceMissing);
                    }
                    reserve_reduction = Some(PieceKind::Duck);
                    PieceOnBoard::new(
                        next_piece_id.inc(),
                        PieceKind::Duck,
                        PieceOrigin::Dropped,
                        PieceForce::Neutral,
                    )
                };
                if new_grid[to].is_some() && mode == TurnMode::Normal {
                    return Err(TurnError::PathBlocked);
                }
                new_grid[to] = Some(duck);
            }
        }
        let facts = TurnFacts {
            castling_relocations,
            next_piece_id,
            reserve_reduction,
            captures,
            steals,
        };
        Ok(TurnOutcome { new_grid, facts })
    }

    // Tells whether `turn` can be executed on the other board.
    pub fn verify_sibling_turn(
        &self, turn: Turn, mode: TurnMode, turn_owner: Force,
    ) -> Result<(), TurnError> {
        if mode == TurnMode::Preturn {
            return Ok(());
        }
        match turn {
            Turn::Move(mv) => {
                if let Some(PromotionTarget::Steal((piece_kind, piece_origin, piece_id))) =
                    mv.promote_to
                {
                    let Some(pos) = find_piece_by_id(&self.grid, piece_id) else {
                        return Err(TurnError::StealTargetMissing);
                    };
                    let Some(piece) = self.grid[pos] else {
                        return Err(TurnError::StealTargetMissing);
                    };
                    if piece.kind != piece_kind {
                        return Err(TurnError::StealTargetInvalid);
                    }
                    if piece.origin != piece_origin {
                        return Err(TurnError::StealTargetInvalid);
                    }
                    return self.stealing_result(pos, turn_owner);
                }
            }
            Turn::Drop(_) => {}
            Turn::Castle(_) => {}
            Turn::PlaceDuck(_) => {}
        }
        Ok(())
    }

    // Applies changes caused by the turn on the other board.
    pub fn apply_sibling_turn(&mut self, facts: &TurnFacts, mode: TurnMode) {
        for capture in &facts.captures {
            assert!(capture.piece_kind.reservable(self.chess_rules()) != PieceReservable::Never);
            // Unwrap ok: duck cannot be captured.
            let force = capture.force.try_into().unwrap();
            self.reserves[force][capture.piece_kind] += 1;
        }
        for steal in &facts.steals {
            let pos = find_piece_by_id(&self.grid, steal.piece_id);
            if mode == TurnMode::Normal {
                // Tested in `verify_sibling_turn`.
                let pos = pos.unwrap();
                assert_eq!(self.grid[pos].unwrap().kind, steal.piece_kind);
                assert_eq!(self.grid[pos].unwrap().force, steal.force);
            }
            if let Some(pos) = pos {
                self.grid[pos] = None;
            }
        }
    }

    // Note. This function should not assume that the turn is valid: it could be a stale preturn.
    fn parse_drag_drop_turn(&self, prototurn: Turn, mode: TurnMode) -> Result<Turn, TurnError> {
        if let Turn::Move(mv) = prototurn {
            match mode {
                TurnMode::Normal => {
                    let force = self.turn_owner(mode);
                    let piece = self.grid[mv.from].ok_or(TurnError::PieceMissing)?;
                    if piece.force != force.into() {
                        return Err(TurnError::WrongTurnOrder);
                    }
                    let first_row = SubjectiveRow::first().to_row(self.shape(), force);
                    if piece.kind == PieceKind::King
                        && mv.from.row == first_row
                        && mv.to.row == first_row
                    {
                        if let Some(dst_piece) = self.grid[mv.to] {
                            if dst_piece.force == force.into() && dst_piece.kind == PieceKind::Rook
                            {
                                if piece.origin == PieceOrigin::Innate {
                                    let castle_direction = match mv.to.col.cmp(&mv.from.col) {
                                        Ordering::Less => CastleDirection::ASide,
                                        Ordering::Greater => CastleDirection::HSide,
                                        Ordering::Equal => {
                                            return Err(TurnError::ImpossibleTrajectory);
                                        }
                                    };
                                    // Castling rights will be checked later when applying the turn.
                                    return Ok(Turn::Castle(castle_direction));
                                } else {
                                    return Err(TurnError::CannotCastleDroppedKing);
                                }
                            }
                        }
                    }
                }
                TurnMode::Preturn => {
                    // Too early to interpret the turn yet.
                }
            }
        }
        Ok(prototurn)
    }

    pub fn algebraic_to_turn(
        &self, algebraic: &AlgebraicTurn, mode: TurnMode, other_board: Option<&Board>,
    ) -> Result<Turn, TurnError> {
        let force = self.turn_owner(mode);
        match algebraic {
            AlgebraicTurn::Move(mv) => {
                let expect_promotion = should_promote(self.shape(), force, mv.piece_kind, mv.to);
                if expect_promotion && mv.promote_to.is_none() {
                    return Err(TurnError::MustPromoteHere);
                }
                if !expect_promotion && mv.promote_to.is_some() {
                    return Err(TurnError::CannotPromoteHere);
                }
                if self.is_duck_turn[force] {
                    return Err(TurnError::MustPlaceDuck);
                }
                let mut turn = None;
                let mut potentially_reachable = false;
                for from in self.shape().coords() {
                    if let Some(piece) = self.grid[from] {
                        if (piece.force == force.into()
                            && piece.kind == mv.piece_kind
                            && mv.from_row.unwrap_or(from.row) == from.row
                            && mv.from_col.unwrap_or(from.col) == from.col)
                        {
                            let reachable;
                            match mode {
                                TurnMode::Normal => {
                                    use Reachability::*;
                                    let capture_or = get_capture(
                                        &self.grid,
                                        from,
                                        mv.to,
                                        self.en_passant_target,
                                    );
                                    match reachability(
                                        self.chess_rules(),
                                        &self.grid,
                                        from,
                                        mv.to,
                                        capture_or.is_some(),
                                    ) {
                                        Reachable => {
                                            if mv.capturing && capture_or.is_none() {
                                                return Err(
                                                    TurnError::CaptureNotationRequiresCapture,
                                                );
                                            }
                                            reachable = true;
                                        }
                                        Blocked => {
                                            potentially_reachable = true;
                                            reachable = false;
                                        }
                                        Impossible => {
                                            reachable = false;
                                        }
                                    }
                                }
                                TurnMode::Preturn => {
                                    reachable = is_reachable_for_premove(
                                        self.chess_rules(),
                                        &self.grid,
                                        from,
                                        mv.to,
                                    )
                                }
                            };
                            if reachable {
                                if turn.is_some() {
                                    // Note. Checking for a preturn may reject a turn that would
                                    // become valid by the time it's executed (because one of the
                                    // pieces that can make the move is blocked or captured, so
                                    // it's no longer ambiguous). However without this condition
                                    // it is unclear how to render the preturn on the client.
                                    return Err(TurnError::AmbiguousNotation);
                                }
                                let promote_to = match mv.promote_to {
                                    Some(AlgebraicPromotionTarget::Upgrade(piece_kind)) => {
                                        Some(PromotionTarget::Upgrade(piece_kind))
                                    }
                                    Some(AlgebraicPromotionTarget::Discard) => {
                                        Some(PromotionTarget::Discard)
                                    }
                                    Some(AlgebraicPromotionTarget::Steal((piece_kind, pos))) => {
                                        let other_board =
                                            other_board.ok_or(TurnError::BadPromotionType)?;
                                        let Some(piece) = other_board.grid[pos] else {
                                            return Err(TurnError::StealTargetMissing);
                                        };
                                        if piece.kind != piece_kind {
                                            return Err(TurnError::StealTargetInvalid);
                                        }
                                        Some(PromotionTarget::Steal((
                                            piece.kind,
                                            piece.origin,
                                            piece.id,
                                        )))
                                    }
                                    None => None,
                                };
                                turn = Some(Turn::Move(TurnMove { from, to: mv.to, promote_to }));
                            }
                        }
                    }
                }
                if let Some(turn) = turn {
                    Ok(turn)
                } else if potentially_reachable {
                    Err(TurnError::PathBlocked)
                } else {
                    Err(TurnError::ImpossibleTrajectory)
                }
            }
            AlgebraicTurn::Drop(drop) => {
                Ok(Turn::Drop(TurnDrop { piece_kind: drop.piece_kind, to: drop.to }))
            }
            AlgebraicTurn::Castle(dir) => Ok(Turn::Castle(*dir)),
            AlgebraicTurn::PlaceDuck(to) => Ok(Turn::PlaceDuck(*to)),
        }
    }

    // Renders turn as algebraic notation, PGN-style, see
    //   http://www.saremba.de/chessgml/standards/pgn/pgn-complete.htm
    //
    // TODO: Check and mate annotations.
    // TODO: Formatting options:
    //   - Short or long algebraic;
    //   - Unicode: None / Just characters / Characters and pieces;
    //   Allow to specify options when exporting PGN.
    pub fn turn_to_algebraic(
        &self, turn: Turn, mode: TurnMode, other_board: Option<&Board>, details: AlgebraicDetails,
    ) -> Option<AlgebraicTurn> {
        let algebraic = self.turn_to_algebraic_impl(turn, mode, other_board, details)?;
        // Improvement potential. Remove when sufficiently tested.
        if let Ok(turn_parsed) = self.algebraic_to_turn(&algebraic, mode, other_board) {
            assert_eq!(turn_parsed, turn, "{:?}", algebraic);
        }
        Some(algebraic)
    }

    fn turn_to_algebraic_impl(
        &self, turn: Turn, mode: TurnMode, other_board: Option<&Board>, details: AlgebraicDetails,
    ) -> Option<AlgebraicTurn> {
        match turn {
            Turn::Move(mv) => {
                let details = match mode {
                    TurnMode::Normal => details,
                    TurnMode::Preturn => AlgebraicDetails::LongAlgebraic,
                };
                let include_col_row = match details {
                    AlgebraicDetails::LongAlgebraic => iproduct!(&[true], &[true]),
                    AlgebraicDetails::ShortAlgebraic => iproduct!(&[false, true], &[false, true]),
                };
                for (&include_col, &include_row) in include_col_row {
                    let piece = self.grid[mv.from]?;
                    let capture = get_capture(&self.grid, mv.from, mv.to, self.en_passant_target);
                    let promote_to = match mv.promote_to {
                        Some(PromotionTarget::Upgrade(piece_kind)) => {
                            Some(AlgebraicPromotionTarget::Upgrade(piece_kind))
                        }
                        Some(PromotionTarget::Discard) => Some(AlgebraicPromotionTarget::Discard),
                        Some(PromotionTarget::Steal((piece_kind, _, piece_id))) => {
                            let other_board = other_board?;
                            let pos = find_piece_by_id(&other_board.grid, piece_id)?;
                            if other_board.grid[pos].unwrap().kind != piece_kind {
                                return None;
                            }
                            Some(AlgebraicPromotionTarget::Steal((piece_kind, pos)))
                        }
                        None => None,
                    };
                    let algebraic = AlgebraicTurn::Move(AlgebraicMove {
                        piece_kind: piece.kind,
                        from_col: if include_col { Some(mv.from.col) } else { None },
                        from_row: if include_row { Some(mv.from.row) } else { None },
                        capturing: capture.is_some(),
                        to: mv.to,
                        promote_to,
                    });
                    if let Ok(turn_parsed) = self.algebraic_to_turn(&algebraic, mode, other_board) {
                        // It's possible that we've got back a different turn if the original turn
                        // was garbage, e.g. c2e4 -> e4 -> e2e4.
                        if turn_parsed == turn {
                            return Some(algebraic);
                        }
                    }
                }
                None
            }
            Turn::Drop(drop) => Some(AlgebraicTurn::Drop(AlgebraicDrop {
                piece_kind: drop.piece_kind,
                to: drop.to,
            })),
            Turn::Castle(dir) => Some(AlgebraicTurn::Castle(dir)),
            Turn::PlaceDuck(to) => Some(AlgebraicTurn::PlaceDuck(to)),
        }
    }
}
