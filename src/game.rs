#![allow(unused_parens)]

use std::rc::Rc;
use std::time::Instant;

use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use lazy_static::lazy_static;
use rand::prelude::*;
use regex::Regex;

use crate::board::{Board, Turn, TurnError, ChessGameStatus, turn_from_algebraic};
use crate::coord::{Row, Col, Coord};
use crate::force::Force;
use crate::grid::Grid;
use crate::piece::{PieceKind, PieceOrigin, PieceOnBoard, CastleDirection};
use crate::rules::{StartingPosition, ChessRules, BughouseRules};


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
    pub fn status(&self) -> ChessGameStatus { self.board.status() }

    pub fn test_flag(&mut self, now: Instant) {
        self.board.test_flag(now);
    }

    // Should `test_flag` first!
    pub fn try_turn(&mut self, turn: Turn, now: Instant) -> Result<(), TurnError> {
        self.board.try_turn(turn, now)?;
        Ok(())
    }
    pub fn try_turn_from_algebraic(&mut self, notation: &str, now: Instant) -> Result<(), TurnError> {
        let active_force = self.board.active_force();
        let turn = turn_from_algebraic(self.board.grid_mut(), active_force, notation)?;
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
        let active_force = self.boards[board_idx].active_force();
        let turn = turn_from_algebraic(self.boards[board_idx].grid_mut(), active_force, notation)?;
        self.try_turn(board_idx, turn, now)
    }

    fn game_status_for_board(&self, board_idx: BughouseBoard) -> BughouseGameStatus {
        use Force::*;
        use BughouseBoard::*;
        match self.boards[board_idx].status() {
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
                board.clock_mut().stop(now);
            }
        }
    }
}
