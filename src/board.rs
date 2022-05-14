#![allow(unused_parens)]

use std::rc::Rc;

use enum_map::{EnumMap, enum_map};
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Serialize, Deserialize};

use crate::coord::{SubjectiveRow, Row, Col, Coord};
use crate::clock::{GameInstant, Clock};
use crate::force::Force;
use crate::grid::Grid;
use crate::piece::{PieceKind, PieceOrigin, PieceOnBoard, CastleDirection, piece_from_algebraic};
use crate::player::Player;
use crate::rules::{DropAggression, ChessRules, BughouseRules};
use crate::util::sort_two;


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
    (col_min.to_zero_based() ..= col_max.to_zero_based()).map(|idx| Col::from_zero_based(idx))
}

fn find_king(grid: &Grid, force: Force) -> Coord {
    for pos in Coord::all() {
        if let Some(piece) = grid[pos] {
            if piece.kind == PieceKind::King && piece.force == force {
                return pos;
            }
        }
    }
    panic!("Cannot find {:?} king", force);
}

fn should_promote(force: Force, piece_kind: PieceKind, to: Coord) -> bool {
    let last_row = SubjectiveRow::from_one_based(8).to_row(force);
    piece_kind == PieceKind::Pawn && to.row == last_row
}

fn can_promote_to(piece_kind: PieceKind) -> bool {
    use PieceKind::*;
    match piece_kind {
        Pawn | King => false,
        Knight | Bishop | Rook | Queen => true,
    }
}

fn get_capture(grid: &Grid, from: Coord, to: Coord, last_turn: &Option<Turn>) -> Option<Coord> {
    let piece = grid[from].unwrap();
    if let Some(target_piece) = grid[to] {
        if target_piece.force == piece.force {
            None
        } else {
            Some(to)
        }
    } else if piece.kind == PieceKind::Pawn {
        if let Some(Turn::Move(last_mv)) = last_turn {
            let last_mv_piece_kind = grid[last_mv.to].unwrap().kind;
            if last_mv_piece_kind == PieceKind::Pawn &&
                last_mv.to.col == to.col &&
                last_mv.from.row - to.row == to.row - last_mv.to.row
            {
                Some(last_mv.to)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    }
}

// Generates move candidates to test whether a player can escape a mate via normal
// chess (not bughouse) moves.
// Simplifications:
//   - Does not generate castles since castling cannot be done while checked.
//   - Pawnes are not promoted.
//   - Drops are not generated (this is done separately in `is_bughouse_mate_to`).
fn generate_moves_for_mate_test(grid: &mut Grid, from: Coord, last_turn: &Option<Turn>) -> Vec<TurnMove> {
    // Improvement potential: Don't iterate over all squares.
    let mut moves = Vec::new();
    for to in Coord::all() {
        let capture_or = get_capture(grid, from, to, last_turn);
        if is_reachable(grid, from, to, capture_or.is_some()) {
            moves.push(TurnMove{ from, to, promote_to: None });
        }
    }
    moves
}

fn king_force(grid: &Grid, king_pos: Coord) -> Force {
    let piece = grid[king_pos].unwrap();
    assert_eq!(piece.kind, PieceKind::King);
    piece.force
}

// Grid is guaratneed to be returned intact.
fn is_chess_mate_to(grid: &mut Grid, king_pos: Coord, last_turn: &Option<Turn>) -> bool {
    if !is_check_to(grid, king_pos) {
        return false;
    }
    let force = king_force(grid, king_pos);
    for pos in Coord::all() {
        if let Some(piece) = grid[pos] {
            if piece.force == force {
                for mv in generate_moves_for_mate_test(grid, pos, last_turn) {
                    let capture_or = get_capture(grid, mv.from, mv.to, last_turn);
                    // Zero out capture separately because of en passant.
                    let mut grid = grid.maybe_scoped_set(capture_or.map(|pos| (pos, None)));
                    let mut grid = grid.scoped_set(mv.from, None);
                    let mut grid = grid.scoped_set(mv.to, Some(piece));
                    let new_king_pos = if piece.kind == PieceKind::King { mv.to } else { king_pos };
                    if !is_check_to(&mut grid, new_king_pos) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// Grid is guaratneed to be returned intact.
fn is_bughouse_mate_to(grid: &mut Grid, king_pos: Coord, last_turn: &Option<Turn>) -> bool {
    let force = king_force(grid, king_pos);
    if !is_chess_mate_to(grid, king_pos, last_turn) {
        return false;
    }
    for pos in Coord::all() {
        if grid[pos].is_none() {
            let mut grid = grid.scoped_set(pos, Some(PieceOnBoard::new(
                PieceKind::Queen, PieceOrigin::Dropped, None, force
            )));
            if !is_check_to(&mut grid, king_pos) {
                return false;
            }
        }
    }
    true
}

fn is_check_to(grid: &Grid, king_pos: Coord) -> bool {
    let force = king_force(grid, king_pos);
    for from in Coord::all() {
        if let Some(piece) = grid[from] {
            if piece.force != force && is_reachable(grid, from, king_pos, true) {
                return true;
            }
        }
    }
    false
}

// Tests that the piece can move in such a way and that the path is free.
// Does not support castling.
fn is_reachable(grid: &Grid, from: Coord, to: Coord, capturing: bool) -> bool {
    if to == from {
        return false;
    }
    let force;
    let piece_kind;
    match grid[from] {
        Some(piece) => {
            force = piece.force;
            piece_kind = piece.kind;
        },
        None => {
            return false;
        },
    }
    if let Some(piece) = grid[to] {
        if piece.force == force {
            return false;
        }
    }
    let (d_row, d_col) = to - from;
    match piece_kind {
        PieceKind::Pawn => {
            let dir_forward = direction_forward(force);
            if capturing {
                d_col.abs() == 1 && d_row == dir_forward
            } else {
                let second_row = SubjectiveRow::from_one_based(2).to_row(force);
                d_col == 0 && (
                    d_row == dir_forward ||
                    (from.row == second_row && d_row == dir_forward * 2)
                )
            }
        },
        PieceKind::Knight => {
            sort_two((d_row.abs(), d_col.abs())) == (1, 2)
        },
        PieceKind::Bishop | PieceKind::Rook | PieceKind::Queen => {
            let is_straight_move = d_row == 0 || d_col == 0;
            let is_diagonal_move = d_row.abs() == d_col.abs();
            if (is_straight_move && piece_kind != PieceKind::Bishop) ||
               (is_diagonal_move && piece_kind != PieceKind::Rook)
            {
                let direction = (d_row.signum(), d_col.signum());
                let mut pos = from + direction;
                while pos != to {
                    if grid[pos].is_some() {
                        return false;
                    }
                    pos = pos + direction;
                }
                true
            } else {
                false
            }
        },
        PieceKind::King => {
            d_row.abs() <= 1 && d_col.abs() <= 1
        },
    }
}

fn as_single_char(s: &str) -> char {
    let mut chars_iter = s.chars();
    let ret = chars_iter.next().unwrap();
    assert!(chars_iter.next().is_none());
    ret
}


#[derive(Clone, Debug)]
pub struct Capture {
    piece_kind: PieceKind,
    force: Force,
}

// Note. Generally speaking, it's impossible to detect castling based on king movement in Chess960.
#[derive(Clone, Copy, Debug)]
pub enum Turn {
    Move(TurnMove),
    Drop(TurnDrop),
    Castle(CastleDirection),
}

#[derive(Clone, Copy, Debug)]
pub struct TurnMove {
    pub from: Coord,
    pub to: Coord,
    pub promote_to: Option<PieceKind>,
}

#[derive(Clone, Copy, Debug)]
pub struct TurnDrop {
    pub piece_kind: PieceKind,
    pub to: Coord,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VictoryReason {
    Checkmate,
    Flag,
    Resignation,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChessGameStatus {
    Active,
    Victory(Force, VictoryReason),
    Draw,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnError {
    InvalidNotation,
    AmbiguousNotation,
    CaptureNotationRequiresCapture,
    PieceMissing,
    WrongTurnOrder,
    Unreachable,
    UnprotectedKing,
    CastlingPieceHasMoved,
    BadPromotion,
    DropFobidden,
    DropPieceMissing,
    DropPosition,
    DropAggression,
    GameOver,
}

pub type Reserve = EnumMap<PieceKind, u8>;

// TODO: Info for draws (number of moves without action; hash map of former positions)
// Improvement potential: Rc => references to a Box in Game classes
#[derive(Clone, Debug)]
pub struct Board {
    #[allow(dead_code)] chess_rules: Rc<ChessRules>,
    bughouse_rules: Option<Rc<BughouseRules>>,
    players: EnumMap<Force, Rc<Player>>,
    status: ChessGameStatus,
    grid: Grid,
    king_has_moved: EnumMap<Force, bool>,
    reserves: EnumMap<Force, Reserve>,
    last_turn: Option<Turn>,  // for en passant capture
    clock: Clock,
    active_force: Force,
}

impl Board {
    pub fn new(
        chess_rules: Rc<ChessRules>,
        bughouse_rules: Option<Rc<BughouseRules>>,
        players: EnumMap<Force, Rc<Player>>,
        starting_grid: Grid,
    ) -> Board {
        let time_control = chess_rules.time_control.clone();
        Board {
            chess_rules,
            bughouse_rules,
            players,
            status: ChessGameStatus::Active,
            grid: starting_grid,
            king_has_moved: enum_map!{ _ => false },
            reserves: enum_map!{ _ => enum_map!{ _ => 0 } },
            last_turn: None,
            clock: Clock::new(time_control),
            active_force: Force::White,
        }
    }

    pub fn player(&self, force: Force) -> &Player { &*self.players[force] }
    pub fn players(&self) -> &EnumMap<Force, Rc<Player>> { &self.players }
    pub fn status(&self) -> ChessGameStatus { self.status }
    pub fn grid(&self) -> &Grid { &self.grid }
    pub fn grid_mut(&mut self) -> &mut Grid { &mut self.grid }
    pub fn reserve(&self, force: Force) -> &Reserve { &self.reserves[force] }
    pub fn reserve_mut(&mut self, force: Force) -> &mut Reserve { &mut self.reserves[force] }
    pub fn reserves(&self) -> &EnumMap<Force, Reserve> { &self.reserves }
    pub fn clock(&self) -> &Clock { &self.clock }
    pub fn clock_mut(&mut self) -> &mut Clock { &mut self.clock }
    pub fn active_force(&self) -> Force { self.active_force }

    fn is_bughouse(&self) -> bool { self.bughouse_rules.is_some() }

    pub fn start_clock(&mut self, now: GameInstant) {
        if !self.clock.is_active() {
            self.clock.new_turn(self.active_force, now);
        }
    }
    pub fn test_flag(&mut self, now: GameInstant) {
        if self.status != ChessGameStatus::Active {
            return;
        }
        if self.clock.time_left(self.active_force, now).is_zero() {
            self.status = ChessGameStatus::Victory(self.active_force.opponent(), VictoryReason::Flag);
        }
    }

    // Does not test flag. Will not update game status if a player has zero time left.
    pub fn try_turn(&mut self, turn: Turn, now: GameInstant) -> Result<Option<Capture>, TurnError> {
        if self.status != ChessGameStatus::Active {
            return Err(TurnError::GameOver);
        }
        let force = self.active_force;
        let (mut new_grid, capture) = self.try_turn_no_check_test(&turn)?;
        let king_pos = find_king(&new_grid, force);
        let opponent_king_pos = find_king(&new_grid, force.opponent());
        if is_check_to(&mut new_grid, king_pos) {
            return Err(TurnError::UnprotectedKing);
        }
        if let Turn::Drop(_) = turn {
            let bughouse_rules = self.bughouse_rules.as_ref().ok_or(TurnError::DropFobidden)?;
            let drop_legal = match bughouse_rules.drop_aggression {
                DropAggression::NoCheck =>
                    !is_check_to(&mut new_grid, opponent_king_pos),
                DropAggression::NoChessMate =>
                    !is_chess_mate_to(&mut new_grid, opponent_king_pos, &self.last_turn),
                DropAggression::NoBughouseMate =>
                    !is_bughouse_mate_to(&mut new_grid, opponent_king_pos, &self.last_turn),
                DropAggression::MateAllowed =>
                    true,
            };
            if !drop_legal {
                return Err(TurnError::DropAggression);
            }
        }
        // Turn validity verified; start updating game state.

        match &turn {
            Turn::Move(mv) => {
                let piece = &mut self.grid[mv.from].unwrap();
                if piece.kind == PieceKind::King {
                    self.king_has_moved[force] = true;
                }
                piece.rook_castling = None;
            },
            Turn::Drop(_) => { },
            Turn::Castle(_) => {
                self.king_has_moved[force] = true;
            }
        }
        self.grid = new_grid;
        if let Turn::Drop(ref drop) = turn {
            let reserve_left = &mut self.reserves[force][drop.piece_kind];
            assert!(*reserve_left > 0);
            *reserve_left -= 1;
        }
        self.last_turn = Some(turn);
        if self.is_bughouse() {
            if is_bughouse_mate_to(&mut self.grid, opponent_king_pos, &self.last_turn) {
                self.status = ChessGameStatus::Victory(force, VictoryReason::Checkmate);
            }
        } else {
            if is_chess_mate_to(&mut self.grid, opponent_king_pos, &self.last_turn) {
                self.status = ChessGameStatus::Victory(force, VictoryReason::Checkmate);
            }
        }
        // TODO: Draw if position is repeated three times.
        self.active_force = force.opponent();
        self.clock.new_turn(self.active_force, now);
        Ok(capture)
    }

    fn try_turn_no_check_test(&self, turn: &Turn) -> Result<(Grid, Option<Capture>), TurnError> {
        let force = self.active_force;
        let mut new_grid = self.grid.clone();
        let mut capture = None;
        match turn {
            Turn::Move(mv) => {
                let piece = new_grid[mv.from].ok_or(TurnError::PieceMissing)?;
                if piece.force != force {
                    return Err(TurnError::WrongTurnOrder);
                }
                let capture_pos_or = get_capture(&new_grid, mv.from, mv.to, &self.last_turn);
                let reachable = is_reachable(&new_grid, mv.from, mv.to, capture_pos_or.is_some());
                if !reachable {
                    return Err(TurnError::Unreachable);
                }
                new_grid[mv.from] = None;
                if let Some(capture_pos) = capture_pos_or {
                    let captured_piece = new_grid[capture_pos].unwrap();
                    capture = Some(Capture {
                        piece_kind: match captured_piece.origin {
                            PieceOrigin::Promoted => PieceKind::Pawn,
                            _ => captured_piece.kind,
                        },
                        force: captured_piece.force
                    });
                    new_grid[capture_pos] = None;
                }
                if should_promote(force, piece.kind, mv.to) {
                    if let Some(promote_to) = mv.promote_to {
                        if can_promote_to(promote_to) {
                            new_grid[mv.to] = Some(PieceOnBoard::new(
                                promote_to, PieceOrigin::Promoted, None, force
                            ));
                        } else {
                            return Err(TurnError::BadPromotion);
                        }
                    } else {
                        return Err(TurnError::BadPromotion);
                    }
                } else {
                    if let Some(_) = mv.promote_to {
                        return Err(TurnError::BadPromotion);
                    } else {
                        new_grid[mv.to] = Some(piece);
                    }
                }
            },
            Turn::Drop(drop) => {
                let bughouse_rules = self.bughouse_rules.as_ref().ok_or(TurnError::DropFobidden)?;
                let to_subjective_row = SubjectiveRow::from_row(drop.to.row, force);
                if drop.piece_kind == PieceKind::Pawn && (
                    to_subjective_row < bughouse_rules.min_pawn_drop_row ||
                    to_subjective_row > bughouse_rules.max_pawn_drop_row
                ) {
                    return Err(TurnError::DropPosition);
                }
                if self.reserves[force][drop.piece_kind] < 1 {
                    return Err(TurnError::DropPieceMissing);
                }
                if new_grid[drop.to].is_some() {
                    return Err(TurnError::DropPosition);
                }
                new_grid[drop.to] = Some(PieceOnBoard::new(
                    drop.piece_kind, PieceOrigin::Dropped, None, force
                ));
            },
            Turn::Castle(dir) => {
                // TODO: More castling tests. Include cases:
                //   - Castle successful.
                //   - Cannot castle when king has moved.
                //   - Cannot castle when rook has moved.
                //   - Cannot castle when there are pieces in between.
                //   - King cannot starts in a checked square.
                //   - King cannot pass through a checked square.
                //   - King cannot ends up in a checked square.
                //   - [Chess960] Castle successful on the first turn.
                //   - [Chess960] Castle blocked by a piece at the destination,
                //      which is outside or kind and rook initial positions.
                //   - [Chess960] Castle when both rooks are on the same side,
                //      both when it's possible (the other rook is further away)
                //      and impossible (the other rook is in the way).
                if self.king_has_moved[force] {
                    return Err(TurnError::CastlingPieceHasMoved);
                }
                let row = SubjectiveRow::from_one_based(1).to_row(force);
                let mut king = None;
                let mut king_pos = None;
                for col in Col::all() {
                    let pos = Coord{ row, col };
                    if let Some(piece) = new_grid[pos] {
                        if piece.force == force && piece.kind == PieceKind::King {
                            king = new_grid[pos].take();
                            king_pos = Some(pos);
                            break;
                        }
                    }
                }
                // King should be in the first row if !self.king_has_moved
                assert!(king.is_some());
                let king_from = king_pos.unwrap();

                let mut rook = None;
                let mut rook_pos = None;
                for col in Col::all() {
                    let pos = Coord{ row, col };
                    if let Some(piece) = new_grid[pos] {
                        if piece.force == force && piece.rook_castling == Some(*dir) {
                            assert_eq!(piece.kind, PieceKind::Rook);
                            rook = new_grid[pos].take();
                            rook.unwrap().rook_castling = None;  // not that is matters, but...
                            rook_pos = Some(pos);
                            break;
                        }
                    }
                }
                if rook.is_none() {
                    return Err(TurnError::CastlingPieceHasMoved);
                }
                let rook_from = rook_pos.unwrap();

                let king_to;
                let rook_to;
                match dir {
                    CastleDirection::ASide => {
                        king_to = Coord::new(row, Col::C);
                        rook_to = Coord::new(row, Col::D);
                    },
                    CastleDirection::HSide => {
                        king_to = Coord::new(row, Col::G);
                        rook_to = Coord::new(row, Col::F);
                    },
                };

                let cols = [king_from.col, king_to.col, rook_from.col, rook_to.col];
                for col in col_range_inclusive(iter_minmax(cols.into_iter()).unwrap()) {
                    if new_grid[Coord::new(row, col)].is_some() {
                        return Err(TurnError::Unreachable);
                    }
                }

                let cols = [king_from.col, king_to.col];
                for col in col_range_inclusive(iter_minmax(cols.into_iter()).unwrap()) {
                    let pos = Coord::new(row, col);
                    let new_grid = new_grid.scoped_set(pos, Some(PieceOnBoard::new(
                        PieceKind::King, PieceOrigin::Innate, None, force
                    )));
                    if is_check_to(&new_grid, pos) {
                        return Err(TurnError::UnprotectedKing);
                    }
                }

                new_grid[king_to] = king;
                new_grid[rook_to] = rook;
            },
        }
        Ok((new_grid, capture))
    }

    pub fn receive_capture(&mut self, capture: &Capture) {
        self.reserves[capture.force][capture.piece_kind] += 1;
    }

    // TODO: Rename to avoid confusion with functions where "make turn" means "apply turn".
    pub fn make_turn_from_algebraic(&self, notation: &str) -> Result<Turn, TurnError> {
        let force = self.active_force;
        let notation = notation.trim();
        const PIECE_RE: &str = r"[PNBRQK]";
        lazy_static! {
            static ref MOVE_RE: Regex = Regex::new(
                &format!(r"^({piece})?([a-h])?([1-8])?([x×:])?([a-h][1-8])(?:[=/]?({piece})?)([+†#‡]?)$", piece=PIECE_RE)
            ).unwrap();
            static ref DROP_RE: Regex = Regex::new(
                &format!(r"^({piece})@([a-h][1-8])$", piece=PIECE_RE)
            ).unwrap();
            static ref A_CASTLING_RE: Regex = Regex::new("^(0-0-0|O-O-O)$").unwrap();
            static ref H_CASTLING_RE: Regex = Regex::new("^(0-0|O-O)$").unwrap();
        }
        if let Some(cap) = MOVE_RE.captures(notation) {
            let piece_kind = cap.get(1).map_or(PieceKind::Pawn, |m| piece_from_algebraic(m.as_str()));
            let from_col = cap.get(2).map(|m| Col::from_algebraic(as_single_char(m.as_str())));
            let from_row = cap.get(3).map(|m| Row::from_algebraic(as_single_char(m.as_str())));
            let capturing = cap.get(4).is_some();
            let to = Coord::from_algebraic(cap.get(5).unwrap().as_str());
            let promote_to = cap.get(6).map(|m| piece_from_algebraic(m.as_str()));
            let _mark = cap.get(7).map(|m| m.as_str());  // TODO: Test check/mate
            if promote_to.is_some() != should_promote(force, piece_kind, to) {
                return Err(TurnError::BadPromotion);
            }
            let mut turn = None;
            for from in Coord::all() {
                if let Some(piece) = self.grid[from] {
                    if (
                        piece.force == force &&
                        piece.kind == piece_kind &&
                        from_row.unwrap_or(from.row) == from.row &&
                        from_col.unwrap_or(from.col) == from.col
                    ) {
                        let capture_or = get_capture(&self.grid, from, to, &self.last_turn);
                        if is_reachable(&self.grid, from, to, capture_or.is_some()) {
                            if capturing && !capture_or.is_some() {
                                return Err(TurnError::CaptureNotationRequiresCapture);
                            }
                            if turn.is_some() {
                                return Err(TurnError::AmbiguousNotation);
                            }
                            turn = Some(Turn::Move(TurnMove{ from, to, promote_to }));
                        }
                    }
                }
            }
            return turn.ok_or(TurnError::Unreachable);
        } else if let Some(cap) = DROP_RE.captures(notation) {
            let piece_kind = piece_from_algebraic(cap.get(1).unwrap().as_str());
            let to = Coord::from_algebraic(cap.get(2).unwrap().as_str());
            return Ok(Turn::Drop(TurnDrop{ piece_kind, to }));
        } else if A_CASTLING_RE.is_match(notation) {
            return Ok(Turn::Castle(CastleDirection::ASide));
        } else if H_CASTLING_RE.is_match(notation) {
            return Ok(Turn::Castle(CastleDirection::HSide));
        }
        Err(TurnError::InvalidNotation)
    }
}
