#![allow(unused_parens)]

use std::rc::Rc;

use enum_map::{enum_map, EnumMap};
use itertools::Itertools;
use rand::prelude::*;

use crate::coord::{SubjectiveRow, Row, Col, Coord};
use crate::force::Force;
use crate::grid::Grid;
use crate::piece::{PieceKind, PieceOrigin, PieceOnBoard, CastleDirection};
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
pub struct ChessRules {
    pub starting_position: StartingPosition,
}

#[derive(Clone, Debug)]
pub struct BughouseRules {
    pub min_pawn_drop_row: SubjectiveRow,
    pub max_pawn_drop_row: SubjectiveRow,
    pub drop_aggression: DropAggression,
}


fn direction_forward(force: Force) -> i8 {
    match force {
        Force::White => -1,
        Force::Black => 1,
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


pub type Reserve = EnumMap<PieceKind, u8>;

// TODO: Info for draws (number of moves without action; hash map of former positions)
pub struct Board {
    #[allow(dead_code)] chess_rules: Rc<ChessRules>,
    bughouse_rules: Option<Rc<BughouseRules>>,
    status: GameStatus,
    grid: Grid,
    // Tells which castling moves can be made based on what pieces have moved (not taking
    // into account checks or the path being occupied).
    castle_rights: EnumMap<Force, EnumMap<CastleDirection, bool>>,
    reserve: EnumMap<Force, Reserve>,
    last_turn: Option<Turn>,  // for en passant capture
    active_force: Force,
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
pub enum GameStatus {
    Active,
    Victory(Force),
    Draw,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnError {
    IllegalChessMove,
    IllegalDropFobidden,
    IllegalDropPieceMissing,
    IllegalDropPosition,
    IllegalDropAggression,
}


impl Board {
    fn new(
        chess_rules: Rc<ChessRules>,
        bughouse_rules: Option<Rc<BughouseRules>>,
        starting_grid: Grid,
    ) -> Board {
        Board {
            chess_rules: chess_rules,
            bughouse_rules: bughouse_rules,
            status: GameStatus::Active,
            grid: starting_grid,
            castle_rights: enum_map!{ _ => enum_map!{ _ => true } },
            reserve: enum_map!{ _ => enum_map!{ _ => 0 } },
            last_turn: None,
            active_force: Force::White,
        }
    }

    fn try_turn(&mut self, turn: Turn) -> Result<Option<Capture>, TurnError> {
        let force = self.active_force;
        let (mut new_grid, capture) = self.try_turn_no_check_test(&turn)?;
        let king_pos = find_king(&new_grid, force);
        let opponent_king_pos = find_king(&new_grid, force.opponent());
        if is_check_to(&mut new_grid, king_pos) {
            return Err(TurnError::IllegalChessMove);
        }
        if let Turn::Drop(_) = turn {
            let bughouse_rules = self.bughouse_rules.as_ref().ok_or(TurnError::IllegalDropFobidden)?;
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
                return Err(TurnError::IllegalDropAggression);
            }
        }

        match &turn {
            Turn::Move(mv) => {
                let piece = self.grid[mv.from].unwrap();
                if piece.kind == PieceKind::King {
                    self.castle_rights[force] = enum_map!{ _ => false };
                } else if let Some(rook_castling) = piece.rook_castling {
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
        self.last_turn = Some(turn);
        if self.bughouse_rules.is_some() {
            if is_bughouse_mate_to(&mut self.grid, opponent_king_pos, &self.last_turn) {
                self.status = GameStatus::Victory(force);
            }
        } else {
            if is_chess_mate_to(&mut self.grid, opponent_king_pos, &self.last_turn) {
                self.status = GameStatus::Victory(force);
            }
        }
        // TODO: Draw if position is repeated three times.
        self.active_force = force.opponent();
        Ok(capture)
    }

    fn try_turn_no_check_test(&self, turn: &Turn) -> Result<(Grid, Option<Capture>), TurnError> {
        let force = self.active_force;
        let mut new_grid = self.grid.clone();
        let mut capture = None;
        match turn {
            Turn::Move(mv) => {
                let capture_pos_or = get_capture(&new_grid, mv.from, mv.to, &self.last_turn);
                let piece = new_grid[mv.from].take().ok_or(TurnError::IllegalChessMove)?;
                if piece.force != force {
                    return Err(TurnError::IllegalChessMove);
                }
                let piece_kind = piece.kind;
                let reachable = is_reachable(
                    &mut new_grid, force, piece_kind,
                    mv.from, mv.to, capture_pos_or.is_some(), false
                );
                if !reachable {
                    return Err(TurnError::IllegalChessMove);
                }
                let last_row = SubjectiveRow::from_one_based(8).to_row(force);
                if mv.to.row == last_row && piece_kind == PieceKind::Pawn {
                    if let Some(promote_to) = mv.promote_to {
                        new_grid[mv.to] = Some(PieceOnBoard::new(
                            promote_to, PieceOrigin::Promoted, None, force
                        ));
                    } else {
                        return Err(TurnError::IllegalChessMove);
                    }
                } else {
                    if let Some(_) = mv.promote_to {
                        return Err(TurnError::IllegalChessMove);
                    } else {
                        new_grid[mv.to] = Some(piece);
                    }
                }
                if let Some(capture_pos) = capture_pos_or {
                    let captured_piece = new_grid[capture_pos].unwrap();
                    capture = Some(Capture{ piece_kind: captured_piece.kind, force: captured_piece.force });
                    new_grid[capture_pos] = None;
                }
            },
            Turn::Drop(drop) => {
                let bughouse_rules = self.bughouse_rules.as_ref().ok_or(TurnError::IllegalDropFobidden)?;
                if drop.piece_kind == PieceKind::Pawn && (
                    drop.to.row < bughouse_rules.min_pawn_drop_row.to_row(force) ||
                    drop.to.row > bughouse_rules.max_pawn_drop_row.to_row(force)
                ) {
                    return Err(TurnError::IllegalDropPosition);
                }
                if self.reserve[force][drop.piece_kind] < 1 {
                    return Err(TurnError::IllegalDropPieceMissing);
                }
                if new_grid[drop.to].is_some() {
                    return Err(TurnError::IllegalDropPosition);
                }
                new_grid[drop.to] = Some(PieceOnBoard::new(
                    drop.piece_kind, PieceOrigin::Dropped, None, force
                ));
            },
            Turn::Castle(dir) => {
                if !self.castle_rights[force][*dir] {
                    return Err(TurnError::IllegalChessMove);
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
                let reachable = is_reachable(
                    &mut new_grid, force, PieceKind::King,
                    king_from, king_to, false, true
                ) && is_reachable(
                    &mut new_grid, force, PieceKind::Rook,
                    rook_from, rook_to, false, true
                );
                if !reachable {
                    return Err(TurnError::IllegalChessMove);
                }
                new_grid[king_to] = king;
                new_grid[rook_to] = rook;
            },
        }
        Ok((new_grid, capture))
    }

    fn receive_capture(&mut self, capture: &Capture) {
        self.reserve[capture.force][capture.piece_kind] += 1;
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

    pub fn try_turn(&mut self, turn: Turn) -> Result<(), TurnError> {
        self.board.try_turn(turn)?;
        Ok(())
    }
}


pub struct BughouseGame {
    boards: [Board; 2],
}

impl BughouseGame {
    pub fn new(chess_rules: ChessRules, bughouse_rules: BughouseRules) -> BughouseGame {
        let starting_position = chess_rules.starting_position;
        let chess_rules = Rc::new(chess_rules);
        let bughouse_rules = Rc::new(bughouse_rules);
        let starting_grid = generate_starting_grid(starting_position);
        let boards = [
            Board::new(Rc::clone(&chess_rules), Some(Rc::clone(&bughouse_rules)), starting_grid.clone()),
            Board::new(Rc::clone(&chess_rules), Some(Rc::clone(&bughouse_rules)), starting_grid),
        ];
        BughouseGame {
            boards: boards,
        }
    }

    pub fn try_turn(&mut self, board_idx: usize, turn: Turn) -> Result<(), TurnError> {
        let capture_or = self.boards[board_idx].try_turn(turn)?;
        if let Some(capture) = capture_or {
            self.boards[1 - board_idx].receive_capture(&capture)
        }
        Ok(())
    }
}
