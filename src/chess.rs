#![allow(unused_parens)]

use std::ops;

use derive_new::new;
use enum_map::{enum_map, Enum, EnumMap};
use itertools::Itertools;

use crate::janitor::Janitor;
use crate::util::sort_two;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartingPosition {
    Classic,
    FischerRandom,  // a.k.a. Chess960
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropAggression {
    NoCheck,
    NoChessMate,
    NoBughouseMate,
    MateAllowed,
}

#[derive(Clone, Debug)]
pub struct BughouseRules {
    pub starting_position: StartingPosition,
    pub min_pawn_drop_row: SubjectiveRow,
    pub max_pawn_drop_row: SubjectiveRow,
    pub drop_aggression: DropAggression,
}


// Row form a force's point of view
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubjectiveRow {
    pub idx: u8,  // 0-based
}
impl SubjectiveRow {
    pub fn from_zero_based(idx: u8) -> Self {
        assert!(idx < NUM_ROWS);
        Self { idx }
    }
    pub fn from_one_based(idx: u8) -> Self {
        Self::from_zero_based((idx).checked_sub(1).unwrap())
    }
    pub fn to_row(self, force: Force) -> Row {
        match force {
            Force::White => Row::from_zero_based(self.idx),
            Force::Black => Row::from_zero_based(NUM_ROWS - self.idx - 1),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Row {
    pub idx: u8,  // 0-based
}
impl Row {
    pub fn from_zero_based(idx: u8) -> Self {
        assert!(idx < NUM_ROWS);
        Self { idx }
    }
    pub fn from_algebraic(idx: char) -> Self {
        Self::from_zero_based((idx as u8).checked_sub('1' as u8).unwrap())
    }
    pub fn to_zero_based(self) -> u8 { self.idx }
    pub fn all() -> impl Iterator<Item = Self> {
        (0..NUM_ROWS).map(|(idx)| Self::from_zero_based(idx))
    }
}
impl ops::Add<i8> for Row {
    type Output = Self;
    fn add(self, other: i8) -> Self::Output {
        Self::from_zero_based((self.to_zero_based() as i8 + other) as u8)
    }
}
impl ops::Sub for Row {
    type Output = i8;
    fn sub(self, other: Self) -> Self::Output {
        (self.to_zero_based() as i8) - (other.to_zero_based() as i8)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Col {
    pub idx: u8,  // 0-based
}
impl Col {
    pub fn from_zero_based(idx: u8) -> Col {
        assert!(idx < NUM_COLS);
        Col { idx }
    }
    pub fn from_algebraic(idx: char) -> Self {
        Self::from_zero_based((idx as u8).checked_sub('A' as u8).unwrap())
    }
    pub fn to_zero_based(self) -> u8 { self.idx }
    pub fn all() -> impl Iterator<Item = Self> {
        (0..NUM_COLS).map(|(idx)| Self::from_zero_based(idx))
    }
}
impl ops::Add<i8> for Col {
    type Output = Self;
    fn add(self, other: i8) -> Self::Output {
        Self::from_zero_based((self.to_zero_based() as i8 + other) as u8)
    }
}
impl ops::Sub for Col {
    type Output = i8;
    fn sub(self, other: Self) -> Self::Output {
        (self.to_zero_based() as i8) - (other.to_zero_based() as i8)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Coord {
    pub row: Row,
    pub col: Col,
}
impl Coord {
    pub fn all() -> impl Iterator<Item = Coord> {
        (0..NUM_ROWS).cartesian_product(0..NUM_COLS).map(|(row_idx, col_idx)|
            Coord{ row: Row::from_zero_based(row_idx), col: Col::from_zero_based(col_idx) }
        )
    }
}
impl ops::Add<(i8, i8)> for Coord {
    type Output = Self;
    fn add(self, other: (i8, i8)) -> Self::Output {
        Self{ row: self.row + other.0, col: self.col + other.1 }
    }
}
impl ops::Sub for Coord {
    type Output = (i8, i8);
    fn sub(self, other: Self) -> Self::Output {
        (self.row - other.row, self.col - other.col)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum Force {
    White,
    Black,
}
impl Force {
    pub fn opponent(self) -> Force {
        match self {
            Force::White => Force::Black,
            Force::Black => Force::White,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceOrigin {
    Innate,
    Promoted,
    Dropped,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new)]
pub struct PieceOnBoard {
    kind: PieceKind,
    origin: PieceOrigin,
    rook_castling: Option<CastleDirection>,  // whether rook can be used to castle
    force: Force,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum CastleDirection {
    ASide,
    HSide,
}

pub type Reserve = EnumMap<PieceKind, u8>;

const NUM_ROWS: u8 = 8;
const NUM_COLS: u8 = 8;


#[derive(Clone, Debug)]
struct Grid {
    data: [[Option<PieceOnBoard>; NUM_COLS as usize]; NUM_ROWS as usize],
}
impl Grid {
    pub fn new() -> Grid {
        Grid { data: Default::default() }
    }
    // Idea. A separate class GridView that allows to make only temporary changes.
    fn maybe_scoped_set(&mut self, change: Option<(Coord, Option<PieceOnBoard>)>)
        -> impl ops::DerefMut<Target = Grid> + '_
    {
        let original = match change {
            None => None,
            Some((pos, new_piece)) => {
                self[pos] = new_piece;
                Some((pos, self[pos]))
            },
        };
        Janitor::new(self, move |grid| {
            if let Some((pos, original_piece)) = original {
                grid[pos] = original_piece;
            }
        })
    }
    fn scoped_set(&mut self, pos: Coord, piece: Option<PieceOnBoard>)
        -> impl ops::DerefMut<Target = Grid> + '_
    {
        let original_piece = self[pos];
        self[pos] = piece;
        Janitor::new(self, move |grid| grid[pos] = original_piece)
    }
}
impl ops::Index<Coord> for Grid {
    type Output = Option<PieceOnBoard>;
    fn index(&self, pos: Coord) -> &Self::Output {
        &self.data[pos.row.idx as usize][pos.col.idx as usize]
    }
}
impl ops::IndexMut<Coord> for Grid {
    fn index_mut(&mut self, pos: Coord) -> &mut Self::Output {
        &mut self.data[pos.row.idx as usize][pos.col.idx as usize]
    }
}

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
    let piece = grid[from].unwrap();
    for to in Coord::all() {
        let capture_or = get_capture(grid, from, to, last_turn);
        if is_reachable(grid, piece.force, piece.kind, from, to, capture_or.is_some(), false) {
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
        let mut grid = grid.scoped_set(pos, Some(PieceOnBoard::new(
            PieceKind::Queen, PieceOrigin::Dropped, None, force
        )));
        if !is_check_to(&mut grid, king_pos) {
            return false;
        }
    }
    true
}

fn is_check_to(grid: &mut Grid, king_pos: Coord) -> bool {
    let force = king_force(grid, king_pos);
    for from in Coord::all() {
        if let Some(piece) = grid[from] {
            if piece.force != force &&
                is_reachable(grid, force, piece.kind, from, king_pos, true, false)
            {
                return true;
            }
        }
    }
    false
}

// Tests that the piece can move in such a way and that the path is free.
// Does *not* test either source or destination square.
// Note: `grid` is guaranteed to be returned intact.
fn is_reachable(
    grid: &mut Grid, force: Force, piece_kind: PieceKind,
    from: Coord, to: Coord, capturing: bool, castling: bool
) -> bool {
    if to == from {
        return false;
    }
    let (d_row, d_col) = from - to;
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
                let mut pos = from;
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
            if castling {
                // Note: not checking `col`, since:
                //   - It can be anything in Chess960,
                //   - Checking whether the king has moved is done separately.
                let first_row = SubjectiveRow::from_one_based(1).to_row(force);
                if from.row != first_row || to.row != first_row {
                    return false;
                }
                let direction = (0, d_col.signum());
                let mut pos = from;
                while pos != to {
                    if grid[pos].is_some() {
                        return false;
                    }
                    let mut grid = grid.scoped_set(pos, Some(PieceOnBoard::new(
                        PieceKind::King, PieceOrigin::Innate, None, force
                    )));
                    if is_check_to(&mut grid, pos) {
                        return false;
                    }
                    pos = pos + direction;
                }
                true
            } else {
                d_row.abs() <= 1 && d_col.abs() <= 1
            }
        },
    }
}


// TODO: Info for draws (number of moves without action; hash map of former positions)
#[derive(Clone, Debug)]
pub struct Board {
    rules: BughouseRules,
    status: GameStatus,
    grid: Grid,
    // Tells which castling moves can be made based on what pieces have moved (not taking
    // into account checks or the path being occupied).
    castle_rights: EnumMap<Force, EnumMap<CastleDirection, bool>>,
    reserve: EnumMap<Force, Reserve>,
    last_turn: Option<Turn>,  // for en passant capture
    active_force: Force,
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
    from: Coord,
    to: Coord,
    promote_to: Option<PieceKind>,
}

#[derive(Clone, Debug)]
pub struct TurnDrop {
    piece_kind: PieceKind,
    to: Coord,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameStatus {
    Active,
    Victory,
    Draw,
}


impl Board {
    pub fn new(rules: BughouseRules) -> Board {
        Board {
            rules: rules,
            status: GameStatus::Active,
            grid: Grid::new(),  // TODO: Generate pieces
            castle_rights: enum_map!{ _ => enum_map!{ _ => true } },
            reserve: enum_map!{ _ => enum_map!{ _ => 0 } },
            last_turn: None,
            active_force: Force::White,
        }
    }

    pub fn try_turn(&mut self, turn: Turn) -> bool {
        let mut new_grid = match self.try_turn_no_check_test(&turn) {
            Some(v) => v,
            None => { return false; },
        };
        let king_pos = find_king(&new_grid, self.active_force);
        let opponent_king_pos = find_king(&new_grid, self.active_force.opponent());
        if is_check_to(&mut new_grid, king_pos) {
            return false;
        }
        if let Turn::Drop(_) = turn {
            let drop_legal = match self.rules.drop_aggression {
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
                return false;
            }
        }

        match &turn {
            Turn::Move(mv) => {
                let piece = self.grid[mv.from].unwrap();
                if piece.kind == PieceKind::King {
                    self.castle_rights[self.active_force] = enum_map!{ _ => false };
                } else if let Some(rook_castling) = piece.rook_castling {
                    assert_eq!(piece.kind, PieceKind::Rook);
                    self.castle_rights[self.active_force][rook_castling] = false;
                }
            },
            Turn::Drop(_) => { },
            Turn::Castle(_) => {
                self.castle_rights[self.active_force] = enum_map!{ _ => false };
            }
        }
        self.grid = new_grid;
        // TODO: Update partner reserve
        self.last_turn = Some(turn);
        if is_bughouse_mate_to(&mut self.grid, opponent_king_pos, &self.last_turn) {
            self.status = GameStatus::Victory;
            return true;
        }
        // TODO: Draw if position is repeated three times.
        self.active_force = self.active_force.opponent();
        true
    }

    fn try_turn_no_check_test(&self, turn: &Turn) -> Option<Grid> {
        let mut new_grid = self.grid.clone();
        match turn {
            Turn::Move(mv) => {
                let piece = new_grid[mv.from].take()?;
                if piece.force != self.active_force {
                    return None;
                }
                let piece_kind = piece.kind;
                let capture_or = get_capture(&new_grid, mv.from, mv.to, &self.last_turn);
                let reachable = is_reachable(
                    &mut new_grid, self.active_force, piece_kind,
                    mv.from, mv.to, capture_or.is_some(), false
                );
                if !reachable {
                    return None;
                }
                let last_row = SubjectiveRow::from_one_based(8).to_row(self.active_force);
                if mv.to.row == last_row && piece_kind == PieceKind::Pawn {
                    if let Some(promote_to) = mv.promote_to {
                        new_grid[mv.to] = Some(PieceOnBoard::new(
                            promote_to, PieceOrigin::Promoted, None, self.active_force
                        ));
                    } else {
                        return None;
                    }
                } else {
                    if let Some(_) = mv.promote_to {
                        return None;
                    } else {
                        new_grid[mv.to] = Some(piece);
                    }
                }
                if let Some(capture) = capture_or {
                    assert!(new_grid[capture].is_some());
                    new_grid[capture] = None;
                }
            },
            Turn::Drop(drop) => {
                if drop.piece_kind == PieceKind::Pawn && (
                    drop.to.row < self.rules.min_pawn_drop_row.to_row(self.active_force) ||
                    drop.to.row > self.rules.max_pawn_drop_row.to_row(self.active_force)
                ) {
                    return None;
                }
                if self.reserve[self.active_force][drop.piece_kind] < 1 {
                    return None;
                }
                if new_grid[drop.to].is_some() {
                    return None;
                }
                new_grid[drop.to] = Some(PieceOnBoard::new(
                    drop.piece_kind, PieceOrigin::Dropped, None, self.active_force
                ));
            },
            Turn::Castle(dir) => {
                if !self.castle_rights[self.active_force][*dir] {
                    return None;
                }
                let row = SubjectiveRow::from_one_based(1).to_row(self.active_force);
                let mut king = None;
                let mut king_pos = None;
                for col in Col::all() {
                    let pos = Coord{ row, col };
                    if let Some(piece) = new_grid[pos] {
                        if piece.force == self.active_force && piece.kind == PieceKind::King {
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
                        if piece.force == self.active_force && piece.rook_castling == Some(*dir) {
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
                        king_to = Coord{ row, col: Col::from_algebraic('C')};
                        rook_to = Coord{ row, col: Col::from_algebraic('D')};
                    },
                    CastleDirection::HSide => {
                        king_to = Coord{ row, col: Col::from_algebraic('G') };
                        rook_to = Coord{ row, col: Col::from_algebraic('F') };
                    },
                };
                let reachable = is_reachable(
                    &mut new_grid, self.active_force, PieceKind::King,
                    king_from, king_to, false, true
                ) && is_reachable(
                    &mut new_grid, self.active_force, PieceKind::Rook,
                    rook_from, rook_to, false, true
                );
                if !reachable {
                    return None;
                }
                new_grid[king_to] = king;
                new_grid[rook_to] = rook;
            },
        }
        Some(new_grid)
    }
}
