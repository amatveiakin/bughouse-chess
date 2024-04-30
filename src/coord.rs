// We use Row/Col terminology instead of traditional Rank/File because "File" could be misleading in
// programming context. But all user-visible places (UI, PGN, etc.) should say Rank/File.
//
// Row, col and coord can take negative or very large values in order to facilitate intermediate
// computations. Valid row and col values start from 0 and are limited by:
//   - board size;
//   - algebraic notation (see `MAX_ROWS` / `MAX_COLS`).
//
// Since `to_algebraic` result is not defined for all values and depends on board size, internal
// serialization can instead use `to_id`, which defines a one-to-one mapping between coords and
// C-identifier strings.

use std::{fmt, ops};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::force::Force;
use crate::once_cell_regex;


// Limited by algebraic notation.
pub const MAX_ROWS: u8 = 20;
pub const MAX_COLS: u8 = 26;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct BoardShape {
    pub num_rows: u8,
    pub num_cols: u8,
}

impl BoardShape {
    pub fn standard() -> Self { BoardShape { num_rows: 8, num_cols: 8 } }
    pub fn contains_row(&self, row: Row) -> bool {
        let row = row.to_zero_based();
        (0..self.num_rows as i8).contains(&row)
    }
    pub fn contains_col(&self, col: Col) -> bool {
        let col = col.to_zero_based();
        (0..self.num_cols as i8).contains(&col)
    }
    pub fn contains_coord(&self, coord: Coord) -> bool {
        self.contains_row(coord.row) && self.contains_col(coord.col)
    }

    pub fn rows(&self) -> impl DoubleEndedIterator<Item = Row> + Clone {
        (0..self.num_rows as i8).map(Row::from_zero_based)
    }
    pub fn cols(&self) -> impl DoubleEndedIterator<Item = Col> + Clone {
        (0..self.num_cols as i8).map(Col::from_zero_based)
    }
    pub fn coords(&self) -> impl Iterator<Item = Coord> + Clone {
        self.rows().cartesian_product(self.cols()).map(|(row, col)| Coord { row, col })
    }
}


// Row form a force's point of view
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct SubjectiveRow {
    idx: i8, // 0-based
}

impl SubjectiveRow {
    pub const fn from_zero_based(idx: i8) -> Self { Self { idx } }
    pub fn from_one_based(idx: i8) -> Self { Self::from_zero_based(idx - 1) }
    pub fn first() -> Self { Self { idx: 0 } }
    pub fn last(board_shape: BoardShape) -> Self { Self { idx: board_shape.num_rows as i8 - 1 } }
    pub const fn to_one_based(&self) -> i8 { self.idx + 1 }
    pub fn to_row(self, board_shape: BoardShape, force: Force) -> Row {
        match force {
            Force::White => Row::from_zero_based(self.idx),
            Force::Black => Row::from_zero_based(board_shape.num_rows as i8 - self.idx - 1),
        }
    }
    pub fn from_row(board_shape: BoardShape, row: Row, force: Force) -> Self {
        match force {
            Force::White => Self::from_zero_based(row.idx),
            Force::Black => Self::from_zero_based(board_shape.num_rows as i8 - row.idx - 1),
        }
    }
}


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Row {
    idx: i8, // 0-based
}

impl Row {
    pub const fn from_zero_based(idx: i8) -> Self { Self { idx } }
    pub fn from_algebraic(idx: char) -> Option<Self> {
        if ('1'..='9').contains(&idx) {
            return Some(Self::from_zero_based(((idx as u32) - ('1' as u32)) as i8));
        }
        if ('①'..='⑳').contains(&idx) {
            return Some(Self::from_zero_based(((idx as u32) - ('①' as u32)) as i8));
        }
        None
    }
    pub const fn to_zero_based(self) -> i8 { self.idx }
    pub fn to_algebraic(self, board_shape: BoardShape) -> char {
        let idx = self.idx;
        assert!(
            0 <= idx && idx < MAX_ROWS as i8 && idx < board_shape.num_rows as i8,
            "{idx} ({board_shape:?})",
        );
        if board_shape.num_rows <= 9 {
            char::from_u32(idx as u32 + '1' as u32).unwrap()
        } else {
            char::from_u32(idx as u32 + '①' as u32).unwrap()
        }
    }
}

impl ops::Add<i8> for Row {
    type Output = Self;
    fn add(self, other: i8) -> Self::Output { Self::from_zero_based(self.to_zero_based() + other) }
}

impl ops::Sub for Row {
    type Output = i8;
    fn sub(self, other: Self) -> Self::Output { self.to_zero_based() - other.to_zero_based() }
}


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Col {
    idx: i8, // 0-based
}

impl Col {
    pub const fn from_zero_based(idx: i8) -> Self { Self { idx } }
    pub fn from_algebraic(idx: char) -> Option<Self> {
        if ('a'..='z').contains(&idx) {
            return Some(Self::from_zero_based(((idx as u32) - ('a' as u32)) as i8));
        }
        None
    }
    pub const fn to_zero_based(self) -> i8 { self.idx }
    pub fn to_algebraic(self, board_shape: BoardShape) -> char {
        let idx = self.idx;
        assert!(
            0 <= idx && idx < MAX_COLS as i8 && idx < board_shape.num_cols as i8,
            "{idx} ({board_shape:?})",
        );
        char::from_u32(idx as u32 + 'a' as u32).unwrap()
    }
}

impl ops::Add<i8> for Col {
    type Output = Self;
    fn add(self, other: i8) -> Self::Output { Self::from_zero_based(self.to_zero_based() + other) }
}

impl ops::Sub for Col {
    type Output = i8;
    fn sub(self, other: Self) -> Self::Output { self.to_zero_based() - other.to_zero_based() }
}


// No `Ord` because there is no single obvious order. Use `Coord::row_col` to compare by row first.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Coord {
    pub row: Row,
    pub col: Col,
}

impl Coord {
    pub const fn new(row: Row, col: Col) -> Self { Self { row, col } }
    pub fn from_algebraic(s: &str) -> Option<Self> {
        let (col, row) = s.chars().collect_tuple()?;
        Some(Coord {
            row: Row::from_algebraic(row)?,
            col: Col::from_algebraic(col)?,
        })
    }
    pub fn from_id(s: &str) -> Option<Self> {
        let re = once_cell_regex!("([pn])([0-9]+)([pn])([0-9]+)");
        let cap = re.captures(s)?;
        let row = Self::from_sign_val(cap.get(1).unwrap().as_str(), cap.get(2).unwrap().as_str())?;
        let col = Self::from_sign_val(cap.get(3).unwrap().as_str(), cap.get(4).unwrap().as_str())?;
        Some(Coord {
            row: Row::from_zero_based(row),
            col: Col::from_zero_based(col),
        })
    }
    pub fn to_algebraic(&self, board_shape: BoardShape) -> String {
        format!("{}{}", self.col.to_algebraic(board_shape), self.row.to_algebraic(board_shape))
    }
    pub fn to_id(self) -> String {
        let (row_sign, row_val) = Self::to_sign_val(self.row.to_zero_based());
        let (col_sign, col_val) = Self::to_sign_val(self.col.to_zero_based());
        format!("{}{}{}{}", row_sign, row_val, col_sign, col_val)
    }
    pub fn row_col(&self) -> (Row, Col) { (self.row, self.col) }
    pub fn color(&self) -> Force {
        if (self.row.to_zero_based() + self.col.to_zero_based()) % 2 == 0 {
            Force::Black
        } else {
            Force::White
        }
    }

    fn from_sign_val(sign: &str, val: &str) -> Option<i8> {
        let val = val.parse::<i8>().ok()?;
        match sign {
            "n" => Some(-val),
            "p" => Some(val),
            _ => None,
        }
    }
    fn to_sign_val(v: i8) -> (char, i8) {
        if v < 0 {
            ('n', -v)
        } else {
            ('p', v)
        }
    }
}

impl ops::Add<(i8, i8)> for Coord {
    type Output = Self;
    fn add(self, other: (i8, i8)) -> Self::Output {
        Self {
            row: (self.row + other.0),
            col: (self.col + other.1),
        }
    }
}

impl ops::Sub for Coord {
    type Output = (i8, i8);
    fn sub(self, other: Self) -> Self::Output { (self.row - other.row, self.col - other.col) }
}

impl fmt::Debug for Coord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Coord({})", self.to_id())
    }
}


// The constants are predefined for the standard 8x8 board.
impl Row {
    #![allow(dead_code)]
    pub const _1: Row = Row { idx: 0 };
    pub const _2: Row = Row { idx: 1 };
    pub const _3: Row = Row { idx: 2 };
    pub const _4: Row = Row { idx: 3 };
    pub const _5: Row = Row { idx: 4 };
    pub const _6: Row = Row { idx: 5 };
    pub const _7: Row = Row { idx: 6 };
    pub const _8: Row = Row { idx: 7 };
}

impl Col {
    #![allow(dead_code)]
    pub const A: Col = Col { idx: 0 };
    pub const B: Col = Col { idx: 1 };
    pub const C: Col = Col { idx: 2 };
    pub const D: Col = Col { idx: 3 };
    pub const E: Col = Col { idx: 4 };
    pub const F: Col = Col { idx: 5 };
    pub const G: Col = Col { idx: 6 };
    pub const H: Col = Col { idx: 7 };
}

impl Coord {
    #![allow(dead_code)]
    pub const A1: Coord = Coord::new(Row::_1, Col::A);
    pub const A2: Coord = Coord::new(Row::_2, Col::A);
    pub const A3: Coord = Coord::new(Row::_3, Col::A);
    pub const A4: Coord = Coord::new(Row::_4, Col::A);
    pub const A5: Coord = Coord::new(Row::_5, Col::A);
    pub const A6: Coord = Coord::new(Row::_6, Col::A);
    pub const A7: Coord = Coord::new(Row::_7, Col::A);
    pub const A8: Coord = Coord::new(Row::_8, Col::A);
    pub const B1: Coord = Coord::new(Row::_1, Col::B);
    pub const B2: Coord = Coord::new(Row::_2, Col::B);
    pub const B3: Coord = Coord::new(Row::_3, Col::B);
    pub const B4: Coord = Coord::new(Row::_4, Col::B);
    pub const B5: Coord = Coord::new(Row::_5, Col::B);
    pub const B6: Coord = Coord::new(Row::_6, Col::B);
    pub const B7: Coord = Coord::new(Row::_7, Col::B);
    pub const B8: Coord = Coord::new(Row::_8, Col::B);
    pub const C1: Coord = Coord::new(Row::_1, Col::C);
    pub const C2: Coord = Coord::new(Row::_2, Col::C);
    pub const C3: Coord = Coord::new(Row::_3, Col::C);
    pub const C4: Coord = Coord::new(Row::_4, Col::C);
    pub const C5: Coord = Coord::new(Row::_5, Col::C);
    pub const C6: Coord = Coord::new(Row::_6, Col::C);
    pub const C7: Coord = Coord::new(Row::_7, Col::C);
    pub const C8: Coord = Coord::new(Row::_8, Col::C);
    pub const D1: Coord = Coord::new(Row::_1, Col::D);
    pub const D2: Coord = Coord::new(Row::_2, Col::D);
    pub const D3: Coord = Coord::new(Row::_3, Col::D);
    pub const D4: Coord = Coord::new(Row::_4, Col::D);
    pub const D5: Coord = Coord::new(Row::_5, Col::D);
    pub const D6: Coord = Coord::new(Row::_6, Col::D);
    pub const D7: Coord = Coord::new(Row::_7, Col::D);
    pub const D8: Coord = Coord::new(Row::_8, Col::D);
    pub const E1: Coord = Coord::new(Row::_1, Col::E);
    pub const E2: Coord = Coord::new(Row::_2, Col::E);
    pub const E3: Coord = Coord::new(Row::_3, Col::E);
    pub const E4: Coord = Coord::new(Row::_4, Col::E);
    pub const E5: Coord = Coord::new(Row::_5, Col::E);
    pub const E6: Coord = Coord::new(Row::_6, Col::E);
    pub const E7: Coord = Coord::new(Row::_7, Col::E);
    pub const E8: Coord = Coord::new(Row::_8, Col::E);
    pub const F1: Coord = Coord::new(Row::_1, Col::F);
    pub const F2: Coord = Coord::new(Row::_2, Col::F);
    pub const F3: Coord = Coord::new(Row::_3, Col::F);
    pub const F4: Coord = Coord::new(Row::_4, Col::F);
    pub const F5: Coord = Coord::new(Row::_5, Col::F);
    pub const F6: Coord = Coord::new(Row::_6, Col::F);
    pub const F7: Coord = Coord::new(Row::_7, Col::F);
    pub const F8: Coord = Coord::new(Row::_8, Col::F);
    pub const G1: Coord = Coord::new(Row::_1, Col::G);
    pub const G2: Coord = Coord::new(Row::_2, Col::G);
    pub const G3: Coord = Coord::new(Row::_3, Col::G);
    pub const G4: Coord = Coord::new(Row::_4, Col::G);
    pub const G5: Coord = Coord::new(Row::_5, Col::G);
    pub const G6: Coord = Coord::new(Row::_6, Col::G);
    pub const G7: Coord = Coord::new(Row::_7, Col::G);
    pub const G8: Coord = Coord::new(Row::_8, Col::G);
    pub const H1: Coord = Coord::new(Row::_1, Col::H);
    pub const H2: Coord = Coord::new(Row::_2, Col::H);
    pub const H3: Coord = Coord::new(Row::_3, Col::H);
    pub const H4: Coord = Coord::new(Row::_4, Col::H);
    pub const H5: Coord = Coord::new(Row::_5, Col::H);
    pub const H6: Coord = Coord::new(Row::_6, Col::H);
    pub const H7: Coord = Coord::new(Row::_7, Col::H);
    pub const H8: Coord = Coord::new(Row::_8, Col::H);
}
