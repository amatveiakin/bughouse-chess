// Improvement potential. Chess draws: dead position, stalemate, fifty-move rule.

#![allow(unused_parens)]

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::mem;

use enum_map::{EnumMap, enum_map};
use itertools::{Itertools, iproduct};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::algebraic::{
    AlgebraicDetails, AlgebraicDrop, AlgebraicMove, AlgebraicPromotionTarget, AlgebraicTurn,
};
use crate::clock::{Clock, GameInstant, TimeMeasurement};
use crate::coord::{BoardShape, Col, Coord, Row, SubjectiveRow};
use crate::force::Force;
use crate::grid::{Grid, GridForRepetitionDraw, GridItem};
use crate::piece::{
    CastleDirection, PieceForRepetitionDraw, PieceForce, PieceId, PieceKind, PieceMovement,
    PieceOnBoard, PieceOrigin, PieceReservable, accolade_combine_pieces,
};
use crate::role::Role;
use crate::rules::{
    BughouseRules, ChessRules, DropAggression, FairyPieces, MatchRules, Promotion, Rules,
};
use crate::starter::{
    BoardSetup, EffectiveStartingPosition, generate_starting_grid, starting_piece_row,
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
    use FairyPieces::*;
    match rules.fairy_pieces {
        NoFairy | Capablanca => None,
        Accolade => accolade_combine_pieces(id, first, second),
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

fn move_destinations(
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

// Generates castling moves. The UI allows to drag the king any number of squares >=2 or even 1
// square if the rook stand right next to it, but we generate one move per direction to reduce
// clutter.
// TODO: Exclude moves when the path is blocked.
fn castling_destinations(
    grid: &Grid, from: Coord, castling_rights: &BoardCastlingRights,
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
    let mut destinations = vec![];
    for (dir, rook_col) in castling_rights[force] {
        if rook_col.is_some() {
            destinations.push(castling_destination(grid.shape(), from, dir))
        }
    }
    destinations
}

fn castling_destination(shape: BoardShape, from: Coord, dir: CastleDirection) -> Coord {
    let d = match dir {
        CastleDirection::ASide => -1,
        CastleDirection::HSide => 1,
    };
    let jump2_col = from.col + d * 2;
    let jump1_col = from.col + d;
    let to_col = if shape.contains_col(jump2_col) {
        jump2_col
    } else {
        jump1_col
    };
    Coord::new(from.row, to_col)
}

// TODO: Exclude moves when the path is blocked.
fn castling_moves(castling_rights: &EnvoyCastlingRights) -> Vec<Turn> {
    castling_rights
        .iter()
        .filter_map(|(dir, col)| col.map(|_| Turn::Castle(dir)))
        .collect()
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
                for to in move_destinations(rules, grid, from, en_passant_target) {
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

fn piece_to_captured(pos: Coord, piece: PieceOnBoard) -> impl Iterator<Item = Capture> {
    let piece_kinds = match piece.origin {
        PieceOrigin::Innate | PieceOrigin::Dropped => vec![piece.kind],
        PieceOrigin::Promoted => vec![PieceKind::Pawn],
        PieceOrigin::Combined((p1, p2)) => vec![p1, p2],
    };
    piece_kinds.into_iter().map(move |piece_kind| Capture {
        from: Some(pos),
        piece_kind,
        force: piece.force,
    })
}

fn initial_castling_rights(
    fairy_pieces: FairyPieces, starting_position: &EffectiveStartingPosition,
) -> EnvoyCastlingRights {
    let row = starting_piece_row(fairy_pieces, starting_position);
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

fn remove_castling_right(
    castling_rights: &mut BoardCastlingRights, board_shape: BoardShape, pos: Coord,
) {
    for force in Force::iter() {
        if pos.row == SubjectiveRow::first().to_row(board_shape, force) {
            for (_, c) in castling_rights[force].iter_mut() {
                if *c == Some(pos.col) {
                    *c = None;
                }
            }
        }
    }
}


// Whether the piece is going to capture. Used by reachability tests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Capturing {
    Yes,   // used by TurnMode::InOrder
    No,    // used by TurnMode::InOrder
    Maybe, // used by TurnMode::Preturn
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reachability {
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capture {
    pub from: Option<Coord>,
    pub piece_kind: PieceKind,
    pub force: PieceForce,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
#[derive(Clone, PartialEq, Eq, Debug)]
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
    // Turn that could be applied immediately.
    InOrder,

    // Potential future turn that is to be executed in due order, but it cannot be made
    // unless something happens on the other board. Engines that consider only one board
    // could compensate lack of full information by coming up with such potential turns
    // (see virtual piece drops in Fairy-Stockfish).
    Virtual,

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
    Checkmate, // means "checkmake", "king lost" (if regicide) or "all kings lost" (if Koedem)
    Flag,
    Resignation,
    UnknownVictory, // for parsing PGN
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DrawReason {
    SimultaneousCheckmate, // for atomic chess
    SimultaneousFlag,      // for bughouse
    ThreefoldRepetition,
    UnknownDraw, // for parsing PGN
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
    DontControlPiece,
    WrongTurnMode,
    InvalidNotation,
    AmbiguousNotation,
    CaptureNotationRequiresCapture,
    PieceMissing,
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
    KingCannotCaptureInAtomicChess,
    MustDropKingIfPossible,
    NoTurnInProgress,
    TurnObsolete,
    PreviousTurnNotFinished,
    Defunct, // turn invalidated by external circumstances, e.g. dragged piece captured
    Cancelled,
    NoGameInProgress,
    GameOver,
    WaybackIsActive,
}

pub type Reserve = EnumMap<PieceKind, u8>;

pub type EnvoyCastlingRights = EnumMap<CastleDirection, Option<Col>>;
pub type BoardCastlingRights = EnumMap<Force, EnvoyCastlingRights>;

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
    castling_rights: BoardCastlingRights,
    en_passant_target: Option<Coord>,
    total_drops: u32,
}


impl Reachability {
    pub fn ok(self) -> bool { self == Reachability::Reachable }
}

// Improvement potential: Don't store players here since they don't affect game process.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Board {
    rules: Rules,
    role: Role,
    player_names: EnumMap<Force, String>,
    status: ChessGameStatus,
    grid: Grid,
    next_piece_id: PieceId,
    // Tracks castling availability based on which pieces have moved. Castling is
    // allowed when the rook stands in the first row at specified columns. If the
    // king has moved then the list is empty. Not affected by temporary limitations
    // (e.g. the king being checked).
    castling_rights: BoardCastlingRights,
    en_passant_target: Option<Coord>,
    reserves: EnumMap<Force, Reserve>,
    total_drops: u32, // total number of drops from both sides
    position_count: HashMap<PositionForRepetitionDraw, u32>,
    clock: Clock,
    full_turn_index: u32, // full index, as in FEN
    active_force: Force,
    is_duck_turn: EnumMap<Force, bool>, // track separately per force to allow preturns
}

impl Board {
    pub fn new(
        rules: Rules, role: Role, players: EnumMap<Force, String>,
        starting_position: &EffectiveStartingPosition,
    ) -> Board {
        let mut next_piece_id = PieceId::new();
        let grid =
            generate_starting_grid(&rules.chess_rules, starting_position, &mut next_piece_id);
        let each_castling_rights =
            initial_castling_rights(rules.chess_rules.fairy_pieces, starting_position);
        let castling_rights = enum_map! { _ => each_castling_rights };
        let reserves = enum_map! { _ => enum_map!{ _ => 0 } };
        let setup = BoardSetup {
            grid,
            next_piece_id,
            castling_rights,
            en_passant_target: None,
            reserves,
            full_turn_index: 1,
            active_force: Force::White,
        };
        Self::new_from_setup(rules, role, players, setup)
    }

    pub fn new_from_setup(
        rules: Rules, role: Role, players: EnumMap<Force, String>, setup: BoardSetup,
    ) -> Board {
        let time_control = rules.chess_rules.time_control.clone();
        let time_measurement = match role {
            Role::ServerOrStandalone => TimeMeasurement::Exact,
            Role::Client => TimeMeasurement::Approximate,
        };
        let mut reserves = setup.reserves;
        if rules.chess_rules.duck_chess {
            let has_duck = reserves.iter().any(|(_, r)| r[PieceKind::Duck] > 0)
                || find_piece(&setup.grid, |p| p.kind == PieceKind::Duck).is_some();
            if !has_duck {
                reserves[Force::White][PieceKind::Duck] = 1;
            }
        }
        let mut board = Board {
            rules,
            role,
            player_names: players,
            status: ChessGameStatus::Active,
            grid: setup.grid,
            next_piece_id: setup.next_piece_id,
            castling_rights: setup.castling_rights,
            en_passant_target: setup.en_passant_target,
            reserves,
            total_drops: 0,
            position_count: HashMap::new(),
            clock: Clock::new(time_control, time_measurement),
            full_turn_index: setup.full_turn_index,
            active_force: setup.active_force,
            is_duck_turn: enum_map! { _ => false },
        };
        board.log_position_for_repetition_draw();
        board
    }

    pub fn new_setup_demo(rules: Rules, role: Role) -> BoardSetup {
        let mut board =
            Board::new(rules, role, Self::stub_players(), &EffectiveStartingPosition::Classic);
        for coord in board.shape().coords() {
            if let Some(piece) = board.grid[coord].take() {
                let force = match piece.force {
                    PieceForce::White | PieceForce::Neutral => Force::White,
                    PieceForce::Black => Force::Black,
                };
                board.reserve_mut(force)[piece.kind] += 1;
            }
        }
        board.into()
    }

    pub fn stub_players() -> EnumMap<Force, String> {
        enum_map! {
            Force::White => "White".to_owned(),
            Force::Black => "Black".to_owned(),
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
    pub fn castling_rights(&self) -> &BoardCastlingRights { &self.castling_rights }
    pub fn en_passant_target(&self) -> Option<Coord> { self.en_passant_target }
    pub fn reserve(&self, force: Force) -> &Reserve { &self.reserves[force] }
    pub fn reserve_mut(&mut self, force: Force) -> &mut Reserve { &mut self.reserves[force] }
    pub fn reserves(&self) -> &EnumMap<Force, Reserve> { &self.reserves }
    pub fn clock(&self) -> &Clock { &self.clock }
    pub fn clock_mut(&mut self) -> &mut Clock { &mut self.clock }
    pub fn full_turn_index(&self) -> u32 { self.full_turn_index }
    pub fn active_force(&self) -> Force { self.active_force }
    pub fn is_duck_turn(&self, force: Force) -> bool { self.is_duck_turn[force] }
    pub fn duck_position(&self) -> Option<Coord> {
        find_piece(&self.grid, |p| p.kind == PieceKind::Duck)
    }

    pub fn is_bughouse(&self) -> bool { self.bughouse_rules().is_some() }
    pub fn turn_owner(&self, mode: TurnMode) -> Force {
        match mode {
            TurnMode::InOrder | TurnMode::Virtual => self.active_force,
            TurnMode::Preturn => self.active_force.opponent(),
        }
    }

    pub fn reset_threefold_repetition_draw(&mut self) { self.position_count.clear(); }

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

    // Checks whether a turn is legal, including check and mate related conditions.
    pub fn is_turn_legal(&self, turn: Turn, mode: TurnMode) -> bool {
        self.turn_outcome(turn, mode).is_ok()
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

    pub fn find_king(&self, force: Force) -> Option<Coord> { find_king(&self.grid, force) }

    pub fn destination_reachability(&self, from: Coord, to: Coord, mode: TurnMode) -> Reachability {
        match mode {
            TurnMode::InOrder | TurnMode::Virtual => {
                let capture = get_capture(&self.grid, from, to, self.en_passant_target);
                reachability(self.chess_rules(), &self.grid, from, to, capture.is_some())
            }
            TurnMode::Preturn => {
                match is_reachable_for_premove(self.chess_rules(), &self.grid, from, to) {
                    true => Reachability::Reachable,
                    false => Reachability::Impossible,
                }
            }
        }
    }

    // Generates legal moves and castlings (if King) for a piece in a given square.
    // Check and mate are not taken into account.
    pub fn turn_destinations(&self, from: Coord) -> Vec<Coord> {
        // TODO: What about preturns? Possibilities:
        //   - Treat as a normal turn (this happens now),
        //   - Include all possibilities,
        //   - Return two separate lists: in-order turn moves + preturn moves.
        let mut ret =
            move_destinations(self.chess_rules(), &self.grid, from, self.en_passant_target);
        ret.extend(castling_destinations(&self.grid, from, &self.castling_rights));
        ret
    }

    // Generates all legal moves and castlings.
    // Check and mate are not taken into account. Filter through `is_turn_legal` if needed.
    // Limitations:
    //   - Always promotes to a Queen.
    //   - Does not support stealing promotion.
    //   - Does not support Duck moves.
    // See also `potential_drops`.
    pub fn potential_moves(&self) -> Vec<Turn> {
        let force = self.active_force;
        let mut ret = vec![];
        for from in self.shape().coords() {
            let Some(piece) = self.grid[from] else {
                continue;
            };
            if piece.force != force.into() {
                continue;
            }
            let destinations =
                move_destinations(self.chess_rules(), &self.grid, from, self.en_passant_target);
            for to in destinations {
                let mut promote_to = None;
                let last_row = SubjectiveRow::last(self.shape()).to_row(self.shape(), force);
                if piece.kind == PieceKind::Pawn && to.row == last_row {
                    promote_to = Some(match self.chess_rules().promotion() {
                        Promotion::Upgrade => PromotionTarget::Upgrade(PieceKind::Queen),
                        Promotion::Discard => PromotionTarget::Discard,
                        Promotion::Steal => {
                            panic!("potential_turns does not support Steal promotion")
                        }
                    })
                }
                ret.push(Turn::Move(TurnMove { from, to, promote_to }));
            }
            if piece.kind == PieceKind::King {
                ret.extend(castling_moves(&self.castling_rights[force]));
            }
        }
        ret
    }

    // Generates all legal drops.
    // Check and mate are not taken into account. Filter through `is_turn_legal` if needed.
    // Limitations:
    //   - Does not generate combining drops in Accolade.
    pub fn potential_drops(&self) -> Vec<Turn> {
        let Some(bughouse_rules) = self.bughouse_rules() else {
            return vec![];
        };
        let force = self.active_force;
        let mut ret = vec![];
        for (piece_kind, &count) in self.reserve(force) {
            if count == 0 {
                continue;
            }
            for to in self.shape().coords() {
                if self.grid[to].is_some() {
                    continue;
                }
                let to_subjective_row = SubjectiveRow::from_row(self.shape(), to.row, force);
                if piece_kind == PieceKind::Pawn
                    && !bughouse_rules.pawn_drop_ranks.contains(to_subjective_row)
                {
                    continue;
                }
                ret.push(Turn::Drop(TurnDrop { piece_kind, to }));
            }
        }
        ret
    }

    pub fn castling_relocation(
        &self, force: Force, dir: CastleDirection,
    ) -> Option<(Coord, Coord)> {
        let from = find_king(&self.grid, force)?;
        let to = castling_destination(self.shape(), from, dir);
        Some((from, to))
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
        if self.role == Role::Client {
            return;
        }
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
            TurnMode::InOrder | TurnMode::Virtual => self.active_force.opponent(),
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
        if (self.active_force == Force::White || mode == TurnMode::Preturn)
            && !self.is_duck_turn[self.active_force]
        {
            self.full_turn_index += 1;
        }
    }

    fn apply_turn(
        &mut self, turn: Turn, mode: TurnMode, new_grid: Grid, facts: &TurnFacts, now: GameInstant,
    ) {
        let shape = self.shape();
        self.next_piece_id = facts.next_piece_id;
        let force = self.turn_owner(mode);
        assert_eq!(self.is_duck_turn[force], matches!(turn, Turn::PlaceDuck(_)));
        match &turn {
            Turn::Move(mv) => {
                remove_castling_right(&mut self.castling_rights, shape, mv.from);
                remove_castling_right(&mut self.castling_rights, shape, mv.to);
            }
            Turn::Drop(drop) => {
                remove_castling_right(&mut self.castling_rights, shape, drop.to);
                self.total_drops += 1;
            }
            Turn::Castle(_) => {
                self.castling_rights[force].clear();
            }
            Turn::PlaceDuck(_) => {}
        }
        for capture in facts.captures.iter() {
            if let Some(capture_pos) = capture.from {
                remove_castling_right(&mut self.castling_rights, shape, capture_pos);
            }
        }
        self.grid = new_grid;
        if let Some(piece_kind) = facts.reserve_reduction {
            // TODO: Properly record negative reserve pieces for virtual turns.
            let reserve_left = &mut self.reserves[force][piece_kind];
            if *reserve_left > 0 {
                *reserve_left -= 1;
            } else {
                match mode {
                    TurnMode::InOrder => panic!("Must have verified reserve earlier"),
                    TurnMode::Virtual | TurnMode::Preturn => {} // ok
                }
            }
        }

        match mode {
            TurnMode::InOrder | TurnMode::Virtual => {
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
                    if self.bughouse_rules().is_some_and(|r| r.koedem) {
                        // Cannot check victory condition here, need to see both boards.
                    } else {
                        let captured_kings = facts
                            .captures
                            .iter()
                            .filter(|c| c.piece_kind == PieceKind::King)
                            .collect_vec();
                        match captured_kings.len() {
                            0 => {}
                            1 => {
                                // Unwrap ok: King cannot be neutral.
                                let loser: Force = captured_kings[0].force.try_into().unwrap();
                                self.status = ChessGameStatus::Victory(
                                    loser.opponent(),
                                    VictoryReason::Checkmate,
                                );
                            }
                            2 => {
                                self.status =
                                    ChessGameStatus::Draw(DrawReason::SimultaneousCheckmate)
                            }
                            _ => panic!(
                                "Shouldn't be able to capture more than two kings if not Koedem"
                            ),
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
            TurnMode::InOrder | TurnMode::Virtual => {
                self.verify_check_and_drop_aggression(turn, mode, &mut outcome)?
            }
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
        let rules = self.chess_rules();
        let bughouse_rules = self.bughouse_rules();
        let force = self.turn_owner(mode);
        if bughouse_rules.is_some_and(|r| r.koedem) && self.reserve(force)[PieceKind::King] > 0 {
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
                    return Err(TurnError::DontControlPiece);
                }
                let mut capture_pos_or = None;
                match mode {
                    TurnMode::InOrder | TurnMode::Virtual => {
                        use Reachability::*;
                        capture_pos_or =
                            get_capture(&new_grid, mv.from, mv.to, self.en_passant_target);
                        match reachability(
                            rules,
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
                        if !is_reachable_for_premove(rules, &new_grid, mv.from, mv.to) {
                            return Err(TurnError::ImpossibleTrajectory);
                        }
                    }
                }
                new_grid[mv.from] = None;
                if let Some(capture_pos) = capture_pos_or {
                    captures.extend(piece_to_captured(capture_pos, new_grid[capture_pos].unwrap()));
                    new_grid[capture_pos] = None;
                    if rules.atomic_chess {
                        if piece.kind == PieceKind::King {
                            // Improvement potential. Should we make an exception for Koedem?
                            return Err(TurnError::KingCannotCaptureInAtomicChess);
                        }
                        for d_row in -1..=1 {
                            for d_col in -1..=1 {
                                let pos = capture_pos + (d_row, d_col);
                                if let GridItem::Piece(&piece) = new_grid.get(pos) {
                                    if piece.kind.destroyed_by_atomic_explosion() {
                                        captures.extend(piece_to_captured(pos, piece));
                                        new_grid[pos] = None;
                                    }
                                }
                            }
                        }
                    }
                }
                // Verify that requested promotion does not violate promotion rules.
                match (mv.promote_to, rules.promotion()) {
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
                            if !promo_piece_kind.can_be_upgrade_promotion_target(rules) {
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
                        if let Some(combined_piece) =
                            combine_pieces(rules, next_piece_id.inc(), piece, dst_piece)
                        {
                            new_grid[mv.to] = Some(combined_piece);
                        } else {
                            assert_eq!(mode, TurnMode::Preturn);
                            new_grid[mv.to] = Some(piece);
                        }
                    } else {
                        new_grid[mv.to] = Some(piece);
                    }
                }
                if rules.atomic_chess && capture_pos_or.is_some() {
                    captures.extend(piece_to_captured(mv.to, new_grid[mv.to].unwrap()));
                    new_grid[mv.to] = None;
                }
            }
            Turn::Drop(drop) => {
                let bughouse_rules = bughouse_rules.ok_or(TurnError::DropRequiresBughouse)?;
                if drop.piece_kind == PieceKind::Duck {
                    return Err(TurnError::DuckPlacementIsSpecialTurnKind);
                }
                if self.is_duck_turn[force] {
                    return Err(TurnError::MustPlaceDuck);
                }
                let to_subjective_row = SubjectiveRow::from_row(self.shape(), drop.to.row, force);
                if drop.piece_kind == PieceKind::Pawn
                    && !bughouse_rules.pawn_drop_ranks.contains(to_subjective_row)
                {
                    return Err(TurnError::InvalidPawnDropRank);
                }
                if self.reserves[force][drop.piece_kind] < 1 {
                    match mode {
                        TurnMode::InOrder => return Err(TurnError::DropPieceMissing),
                        TurnMode::Virtual | TurnMode::Preturn => {}
                    }
                }
                let piece_force = drop.piece_kind.reserve_piece_force(force);
                let mut new_piece = PieceOnBoard::new(
                    next_piece_id.inc(),
                    drop.piece_kind,
                    PieceOrigin::Dropped,
                    piece_force,
                );
                if let Some(dst_piece) = new_grid[drop.to] {
                    if let Some(combined_piece) =
                        combine_pieces(rules, next_piece_id.inc(), new_piece, dst_piece)
                    {
                        new_piece = combined_piece;
                    } else {
                        match mode {
                            TurnMode::InOrder | TurnMode::Virtual => {
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

                let (king_to_col, rook_to_col) = match rules.fairy_pieces {
                    FairyPieces::NoFairy | FairyPieces::Accolade => match dir {
                        CastleDirection::ASide => (Col::C, Col::D),
                        CastleDirection::HSide => (Col::G, Col::F),
                    },
                    FairyPieces::Capablanca => match dir {
                        CastleDirection::ASide => (Col::C, Col::D),
                        CastleDirection::HSide => (Col::I, Col::H),
                    },
                };
                let king_to = Coord::new(row, king_to_col);
                let rook_to = Coord::new(row, rook_to_col);

                match mode {
                    TurnMode::InOrder | TurnMode::Virtual => {
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
                            if !rules.regicide() && is_check_to(rules, &new_grid, pos) {
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
                if !rules.duck_chess {
                    return Err(TurnError::NotDuckChess);
                }
                if !self.is_duck_turn[force] {
                    return Err(TurnError::MustMovePieceBeforeDuck);
                }
                let from = find_piece(&new_grid, |p| p.kind == PieceKind::Duck);
                let duck = if let Some(from) = from {
                    if to == from {
                        match mode {
                            TurnMode::InOrder | TurnMode::Virtual => {
                                return Err(TurnError::MustChangeDuckPosition);
                            }
                            TurnMode::Preturn => {}
                        }
                    }
                    new_grid[from].take().unwrap()
                } else {
                    if self.reserves[force][PieceKind::Duck] == 0 {
                        // This shouldn't really happen. This isn't a legal virtual drop either,
                        // because you cannot expect to get a duck from the other board.
                        match mode {
                            TurnMode::InOrder | TurnMode::Virtual => {
                                return Err(TurnError::DropPieceMissing);
                            }
                            TurnMode::Preturn => {}
                        }
                    }
                    reserve_reduction = Some(PieceKind::Duck);
                    PieceOnBoard::new(
                        next_piece_id.inc(),
                        PieceKind::Duck,
                        PieceOrigin::Dropped,
                        PieceForce::Neutral,
                    )
                };
                if new_grid[to].is_some() {
                    match mode {
                        TurnMode::InOrder | TurnMode::Virtual => {
                            return Err(TurnError::PathBlocked);
                        }
                        TurnMode::Preturn => {}
                    }
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
        match mode {
            TurnMode::InOrder => {}
            TurnMode::Virtual | TurnMode::Preturn => return Ok(()),
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

    pub fn check_duplicate(&mut self, facts: &TurnFacts) {
        for capture in &facts.captures {
            assert!(capture.piece_kind.reservable(self.chess_rules()) != PieceReservable::Never);
            // Unwrap ok: duck cannot be captured.
            let force: Force = capture.force.try_into().unwrap();
            self.reserves[force.opponent()][capture.piece_kind] += 1;
        }
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
            match mode {
                TurnMode::InOrder | TurnMode::Virtual => {
                    // Tested in `verify_sibling_turn`.
                    let pos = pos.unwrap();
                    assert_eq!(self.grid[pos].unwrap().kind, steal.piece_kind);
                    assert_eq!(self.grid[pos].unwrap().force, steal.force);
                }
                TurnMode::Preturn => {}
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
                TurnMode::InOrder | TurnMode::Virtual => {
                    let force = self.turn_owner(mode);
                    let piece = self.grid[mv.from].ok_or(TurnError::PieceMissing)?;
                    if piece.force != force.into() {
                        return Err(TurnError::DontControlPiece);
                    }
                    let first_row = SubjectiveRow::first().to_row(self.shape(), force);
                    let d_col = mv.from.col - mv.to.col;
                    let onto_rook = self.grid[mv.to].is_some_and(|dst_piece| {
                        dst_piece.force == force.into() && dst_piece.kind == PieceKind::Rook
                    });
                    let is_castling = piece.kind == PieceKind::King
                        && ((d_col.abs() >= 2) || onto_rook)
                        && (mv.from.row == first_row && mv.to.row == first_row);
                    if is_castling {
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
                                TurnMode::InOrder | TurnMode::Virtual => {
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
                    TurnMode::InOrder | TurnMode::Virtual => details,
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

impl From<Board> for BoardSetup {
    fn from(board: Board) -> BoardSetup {
        BoardSetup {
            grid: board.grid,
            next_piece_id: board.next_piece_id,
            castling_rights: board.castling_rights,
            en_passant_target: board.en_passant_target,
            reserves: board.reserves,
            full_turn_index: board.full_turn_index,
            active_force: board.active_force,
        }
    }
}
