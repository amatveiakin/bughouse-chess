#![allow(unused_parens)]

use std::cmp;
use std::rc::Rc;
use std::time::Instant;

use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use lazy_static::lazy_static;
use rand::prelude::*;
use regex::Regex;

use crate::coord::{SubjectiveRow, Row, Col, Coord};
use crate::clock::Clock;
use crate::force::Force;
use crate::grid::Grid;
use crate::piece::{PieceKind, PieceOrigin, PieceOnBoard, CastleDirection};
use crate::rules::{DropAggression, StartingPosition, ChessRules, BughouseRules};
use crate::util::sort_two;


fn direction_forward(force: Force) -> i8 {
    match force {
        Force::White => 1,
        Force::Black => -1,
    }
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
    // TODO: Optimize: don't iterate over all squares
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
// TODO: Consider alternative way of supporting en passant: return enum
//   (Yes / No / If en passant).
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

fn piece_from_algebraic(notation: &str) -> PieceKind {
    match notation {
        "P" => PieceKind::Pawn,
        "N" => PieceKind::Knight,
        "B" => PieceKind::Bishop,
        "R" => PieceKind::Rook,
        "Q" => PieceKind::Queen,
        "K" => PieceKind::King,
        _ => panic!("Unknown piece: {}", notation),
    }
}

fn as_single_char(s: &str) -> char {
    let mut chars_iter = s.chars();
    let ret = chars_iter.next().unwrap();
    assert!(chars_iter.next().is_none());
    ret
}

fn turn_from_algebraic(grid: &mut Grid, force: Force, notation: &str) -> Result<Turn, TurnError> {
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
            if let Some(piece) = grid[from] {
                if (
                    piece.force == force &&
                    piece.kind == piece_kind &&
                    from_row.unwrap_or(from.row) == from.row &&
                    from_col.unwrap_or(from.col) == from.col
                ) {
                    // TODO: Proper capture checks
                    if is_reachable(grid, from, to, capturing) {
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


#[derive(Clone, Debug)]
pub struct Capture {
    piece_kind: PieceKind,
    force: Force,
}

// Note. Generally speaking, it's impossible to detect castling based on king movement in Chess960.
#[derive(Clone, Debug)]
pub enum Turn {
    Move(TurnMove),
    Drop(TurnDrop),
    Castle(CastleDirection),
}

#[derive(Clone, Debug)]
pub struct TurnMove {
    pub from: Coord,
    pub to: Coord,
    pub promote_to: Option<PieceKind>,
}

#[derive(Clone, Debug)]
pub struct TurnDrop {
    pub piece_kind: PieceKind,
    pub to: Coord,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChessGameStatus {
    Active,
    Victory(Force),
    Draw,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum BughouseBoard {
    A,
    B,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BughouseTeam {
    First,
    Second,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BughouseGameStatus {
    Active,
    Victory(BughouseTeam),
    Draw,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnError {
    InvalidNotation,
    AmbiguousNotation,
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

impl BughouseBoard {
    pub fn other(self) -> Self {
        match self {
            BughouseBoard::A => BughouseBoard::B,
            BughouseBoard::B => BughouseBoard::A,
        }
    }
}

impl BughouseTeam {
    pub fn opponent(self) -> Self {
        match self {
            BughouseTeam::First => BughouseTeam::Second,
            BughouseTeam::Second => BughouseTeam::First,
        }
    }
}

pub type Reserve = EnumMap<PieceKind, u8>;

// TODO: Info for draws (number of moves without action; hash map of former positions)
// TODO: Rc => references to a Box in Game classes
pub struct Board {
    #[allow(dead_code)] chess_rules: Rc<ChessRules>,
    bughouse_rules: Option<Rc<BughouseRules>>,
    status: ChessGameStatus,
    grid: Grid,
    // Tells which castling moves can be made based on what pieces have moved (not taking
    // into account checks or the path being occupied).
    castle_rights: EnumMap<Force, EnumMap<CastleDirection, bool>>,
    reserves: EnumMap<Force, Reserve>,
    last_turn: Option<Turn>,  // for en passant capture
    clock: Clock,
    active_force: Force,
}

impl Board {
    fn new(
        chess_rules: Rc<ChessRules>,
        bughouse_rules: Option<Rc<BughouseRules>>,
        starting_grid: Grid,
    ) -> Board {
        let time_control = chess_rules.time_control.clone();
        Board {
            chess_rules: chess_rules,
            bughouse_rules: bughouse_rules,
            status: ChessGameStatus::Active,
            grid: starting_grid,
            castle_rights: enum_map!{ _ => enum_map!{ _ => true } },
            reserves: enum_map!{ _ => enum_map!{ _ => 0 } },
            last_turn: None,
            clock: Clock::new(time_control),
            active_force: Force::White,
        }
    }

    pub fn grid(&self) -> &Grid { &self.grid }
    pub fn reserve(&self, force: Force) -> &Reserve { &self.reserves[force] }
    pub fn reserves(&self) -> &EnumMap<Force, Reserve> { &self.reserves }
    pub fn clock(&self) -> &Clock { &self.clock }

    fn is_bughouse(&self) -> bool { self.bughouse_rules.is_some() }

    fn start_clock(&mut self, now: Instant) {
        if !self.clock.is_active() {
            self.clock.new_turn(self.active_force, now);
        }
    }
    fn test_flag(&mut self, now: Instant) {
        if self.status != ChessGameStatus::Active {
            return;
        }
        if self.clock.time_left(self.active_force, now).is_zero() {
            self.status = ChessGameStatus::Victory(self.active_force.opponent());
        }
    }

    fn try_turn(&mut self, turn: Turn, now: Instant) -> Result<Option<Capture>, TurnError> {
        self.test_flag(now);
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
                let piece = self.grid[mv.from].unwrap();
                if piece.kind == PieceKind::King {
                    self.castle_rights[force] = enum_map!{ _ => false };
                } else if let Some(rook_castling) = piece.rook_castling {
                    // TODO: FIX: Also remove castle right when rook is captured
                    assert_eq!(piece.kind, PieceKind::Rook);
                    self.castle_rights[force][rook_castling] = false;
                }
            },
            Turn::Drop(_) => { },
            Turn::Castle(_) => {
                self.castle_rights[force] = enum_map!{ _ => false };
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
                self.status = ChessGameStatus::Victory(force);
            }
        } else {
            if is_chess_mate_to(&mut self.grid, opponent_king_pos, &self.last_turn) {
                self.status = ChessGameStatus::Victory(force);
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
                if !self.castle_rights[force][*dir] {
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
                // Shouldn't have castle right if the king has moved.
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
                            rook_pos = Some(pos);
                            break;
                        }
                    }
                }
                // Shouldn't have castle right if the rook has moved.
                assert!(rook.is_some());
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
                let mut col = *cols.iter().min().unwrap();
                let max_col = *cols.iter().max().unwrap();
                while col != max_col {
                    if new_grid[Coord::new(row, col)].is_some() {
                        return Err(TurnError::Unreachable);
                    }
                    col = col + 1;
                }

                let mut col = cmp::min(king_from.col, king_to.col);
                let max_col = cmp::max(king_from.col, king_to.col);
                while col != max_col {
                    let pos = Coord::new(row, col);
                    let new_grid = new_grid.scoped_set(pos, Some(PieceOnBoard::new(
                        PieceKind::King, PieceOrigin::Innate, None, force
                    )));
                    if is_check_to(&new_grid, pos) {
                        return Err(TurnError::UnprotectedKing);
                    }
                    col = col + 1;
                }

                new_grid[king_to] = king;
                new_grid[rook_to] = rook;
            },
        }
        Ok((new_grid, capture))
    }

    fn receive_capture(&mut self, capture: &Capture) {
        self.reserves[capture.force][capture.piece_kind] += 1;
    }
}


fn generate_starting_grid(starting_position: StartingPosition) -> Grid {
    use CastleDirection::*;
    use PieceKind::*;
    let new_white = |kind| {
        assert_ne!(kind, Rook);
        PieceOnBoard::new(kind, PieceOrigin::Innate, None, Force::White)
    };
    let new_white_rook = |castling| {
        PieceOnBoard::new(Rook, PieceOrigin::Innate, Some(castling), Force::White)
    };
    let mut grid = Grid::new();

    for col in Col::all() {
        grid[Coord::new(Row::_2, col)] = Some(new_white(Pawn));
    }
    match starting_position {
        StartingPosition::Classic => {
            grid[Coord::A1] = Some(new_white_rook(ASide));
            grid[Coord::B1] = Some(new_white(Knight));
            grid[Coord::C1] = Some(new_white(Bishop));
            grid[Coord::D1] = Some(new_white(Queen));
            grid[Coord::E1] = Some(new_white(King));
            grid[Coord::F1] = Some(new_white(Bishop));
            grid[Coord::G1] = Some(new_white(Knight));
            grid[Coord::H1] = Some(new_white_rook(HSide));
        },
        StartingPosition::FischerRandom => {
            let mut rng = rand::thread_rng();
            let row = Row::_1;
            grid[Coord::new(row, Col::from_zero_based(rng.gen_range(0..4) * 2))] = Some(new_white(Bishop));
            grid[Coord::new(row, Col::from_zero_based(rng.gen_range(0..4) * 2 + 1))] = Some(new_white(Bishop));
            let mut cols = Col::all().filter(|col| grid[Coord::new(row, *col)].is_none()).collect_vec();
            cols.shuffle(&mut rng);
            let (king_and_rook_cols, queen_and_knight_cols) = cols.split_at(3);
            let [&left_rook_col, &king_col, &right_rook_col] =
                <[&Col; 3]>::try_from(king_and_rook_cols.into_iter().sorted().collect_vec()).unwrap();
            let [queen_col, knight_col_1, knight_col_2] =
                <[Col; 3]>::try_from(queen_and_knight_cols).unwrap();
            grid[Coord::new(row, left_rook_col)] = Some(new_white_rook(ASide));
            grid[Coord::new(row, king_col)] = Some(new_white(King));
            grid[Coord::new(row, right_rook_col)] = Some(new_white_rook(HSide));
            grid[Coord::new(row, queen_col)] = Some(new_white(Queen));
            grid[Coord::new(row, knight_col_1)] = Some(new_white(Knight));
            grid[Coord::new(row, knight_col_2)] = Some(new_white(Knight));
        },
    }

    for col in Col::all() {
        grid[Coord::new(Row::_7, col)] = grid[Coord::new(Row::_2, col)].map(|mut piece| {
            piece.force = Force::Black;
            piece
        });
        grid[Coord::new(Row::_8, col)] = grid[Coord::new(Row::_1, col)].map(|mut piece| {
            piece.force = Force::Black;
            piece
        });
    }
    grid
}

pub struct ChessGame {
    board: Board,
}

impl ChessGame {
    pub fn new(rules: ChessRules) -> ChessGame {
        let starting_position = rules.starting_position;
        ChessGame {
            board: Board::new(Rc::new(rules), None, generate_starting_grid(starting_position)),
        }
    }

    pub fn board(&self) -> &Board { &self.board }
    pub fn status(&self) -> ChessGameStatus { self.board.status }

    pub fn test_flag(&mut self, now: Instant) {
        self.board.test_flag(now);
    }

    // Should `test_flag` first!
    pub fn try_turn(&mut self, turn: Turn, now: Instant) -> Result<(), TurnError> {
        self.board.try_turn(turn, now)?;
        Ok(())
    }
    pub fn try_turn_from_algebraic(&mut self, notation: &str, now: Instant) -> Result<(), TurnError> {
        let active_force = self.board.active_force;
        let turn = turn_from_algebraic(&mut self.board.grid, active_force, notation)?;
        self.try_turn(turn, now)
    }
    pub fn try_replay_log(&mut self, log: &str) -> Result<(), TurnError> {
        lazy_static! {
            static ref TURN_NUMBER_RE: Regex = Regex::new(r"^(?:[0-9]+\.)?(.*)$").unwrap();
        }
        // TODO: What should happen to time when replaying log?
        let now = Instant::now();
        for turn_notation in log.split_whitespace() {
            let turn_notation = TURN_NUMBER_RE.captures(turn_notation).unwrap().get(1).unwrap().as_str();
            self.try_turn_from_algebraic(turn_notation, now)?
        }
        Ok(())
    }
}


pub struct BughouseGame {
    boards: EnumMap<BughouseBoard, Board>,
    status: BughouseGameStatus,
}

impl BughouseGame {
    pub fn new(chess_rules: ChessRules, bughouse_rules: BughouseRules) -> BughouseGame {
        let starting_position = chess_rules.starting_position;
        let chess_rules = Rc::new(chess_rules);
        let bughouse_rules = Rc::new(bughouse_rules);
        let starting_grid = generate_starting_grid(starting_position);
        let boards = enum_map!{
            _ => Board::new(Rc::clone(&chess_rules), Some(Rc::clone(&bughouse_rules)), starting_grid.clone())
        };
        BughouseGame {
            boards: boards,
            status: BughouseGameStatus::Active,
        }
    }

    pub fn board(&self, idx: BughouseBoard) -> &Board { &self.boards[idx] }
    pub fn status(&self) -> BughouseGameStatus { self.status }

    pub fn test_flag(&mut self, now: Instant) {
        use BughouseBoard::*;
        use BughouseGameStatus::*;
        self.boards[A].test_flag(now);
        self.boards[B].test_flag(now);
        let status_a = self.game_status_for_board(A);
        let status_b = self.game_status_for_board(B);
        let status = match (status_a, status_b) {
            (Victory(victory_a), Victory(victory_b)) => {
                if victory_a == victory_b { Victory(victory_a) } else { Draw }
            },
            (Victory(victory), Active) => { Victory(victory) },
            (Active, Victory(victory)) => { Victory(victory) },
            (Active, Active) => { Active },
            (Draw, _) | (_, Draw) => {
                panic!("Cannot draw on flag");
            }
        };
        self.set_status(status, now);
    }

    // Should `test_flag` first!
    pub fn try_turn(&mut self, board_idx: BughouseBoard, turn: Turn, now: Instant)
        -> Result<(), TurnError>
    {
        let capture_or = self.boards[board_idx].try_turn(turn, now)?;
        self.boards[board_idx.other()].start_clock(now);
        if let Some(capture) = capture_or {
            self.boards[board_idx.other()].receive_capture(&capture)
        }
        assert!(self.status == BughouseGameStatus::Active);
        self.set_status(self.game_status_for_board(board_idx), now);
        Ok(())
    }
    pub fn try_turn_from_algebraic(&mut self, board_idx: BughouseBoard, notation: &str, now: Instant)
        -> Result<(), TurnError>
    {
        let active_force = self.boards[board_idx].active_force;
        let turn = turn_from_algebraic(&mut self.boards[board_idx].grid, active_force, notation)?;
        self.try_turn(board_idx, turn, now)
    }

    fn game_status_for_board(&self, board_idx: BughouseBoard) -> BughouseGameStatus {
        use Force::*;
        use BughouseBoard::*;
        match self.boards[board_idx].status {
            ChessGameStatus::Active => BughouseGameStatus::Active,
            ChessGameStatus::Victory(force) => {
                BughouseGameStatus::Victory(match (board_idx, force) {
                    (A, White) | (B, Black) => BughouseTeam::First,
                    (B, White) | (A, Black) => BughouseTeam::Second,
                })
            },
            ChessGameStatus::Draw => BughouseGameStatus::Draw,
        }
    }
    fn set_status(&mut self, status: BughouseGameStatus, now: Instant) {
        self.status = status;
        if status != BughouseGameStatus::Active {
            for (_, board) in self.boards.iter_mut() {
                board.clock.stop(now);
            }
        }
    }
}
