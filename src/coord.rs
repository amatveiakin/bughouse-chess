// We use Row/Col terminology instead of traditional Rank/File because "File" could be misleading
// in programming context. But all user-visible places (UI, PGN, etc.) should say Rank/File.

// Rust-upgrade(https://github.com/rust-lang/rust/issues/91917):
//   Use `then_some` in `from_zero_based`.

use std::{fmt, ops};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::force::Force;


pub const NUM_ROWS: u8 = 8;
pub const NUM_COLS: u8 = 8;


// Row form a force's point of view
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct SubjectiveRow {
    idx: u8, // 0-based
}

impl SubjectiveRow {
    pub const fn from_zero_based(idx: u8) -> Option<Self> {
        if idx < NUM_ROWS {
            Some(Self { idx })
        } else {
            None
        }
    }
    pub fn from_one_based(idx: u8) -> Option<Self> {
        (idx).checked_sub(1).and_then(Self::from_zero_based)
    }
    pub const fn to_one_based(&self) -> u8 { self.idx + 1 }
    pub fn to_row(self, force: Force) -> Row {
        match force {
            Force::White => Row::from_zero_based(self.idx).unwrap(),
            Force::Black => Row::from_zero_based(NUM_ROWS - self.idx - 1).unwrap(),
        }
    }
    pub fn from_row(row: Row, force: Force) -> Self {
        match force {
            Force::White => Self::from_zero_based(row.idx).unwrap(),
            Force::Black => Self::from_zero_based(NUM_ROWS - row.idx - 1).unwrap(),
        }
    }
}


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Row {
    idx: u8, // 0-based
}

impl Row {
    pub const fn from_zero_based(idx: u8) -> Option<Self> {
        if idx < NUM_ROWS {
            Some(Self { idx })
        } else {
            None
        }
    }
    pub fn from_algebraic(idx: char) -> Option<Self> {
        (idx as u8).checked_sub(b'1').and_then(Self::from_zero_based)
    }
    pub const fn to_zero_based(self) -> u8 { self.idx }
    pub const fn to_algebraic(self) -> char { (self.idx + b'1') as char }
    pub fn all() -> impl DoubleEndedIterator<Item = Self> + Clone {
        (0..NUM_ROWS).map(|v| Self::from_zero_based(v).unwrap())
    }
}

impl ops::Add<i8> for Row {
    type Output = Option<Self>;
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


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Col {
    idx: u8, // 0-based
}

impl Col {
    pub const fn from_zero_based(idx: u8) -> Option<Self> {
        if idx < NUM_COLS {
            Some(Self { idx })
        } else {
            None
        }
    }
    pub fn from_algebraic(idx: char) -> Option<Self> {
        (idx as u8).checked_sub(b'a').and_then(Self::from_zero_based)
    }
    pub const fn to_zero_based(self) -> u8 { self.idx }
    pub const fn to_algebraic(self) -> char { (self.idx + b'a') as char }
    pub fn all() -> impl DoubleEndedIterator<Item = Self> + Clone {
        (0..NUM_COLS).map(|v| Self::from_zero_based(v).unwrap())
    }
}

impl ops::Add<i8> for Col {
    type Output = Option<Self>;
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
    pub fn to_algebraic(&self) -> String {
        format!("{}{}", self.col.to_algebraic(), self.row.to_algebraic())
    }
    pub fn row_col(&self) -> (Row, Col) { (self.row, self.col) }
    pub fn all() -> impl Iterator<Item = Coord> {
        Row::all().cartesian_product(Col::all()).map(|(row, col)| Coord { row, col })
    }
}

impl ops::Add<(i8, i8)> for Coord {
    type Output = Option<Self>;
    fn add(self, other: (i8, i8)) -> Self::Output {
        Some(Self {
            row: (self.row + other.0)?,
            col: (self.col + other.1)?,
        })
    }
}

impl ops::Sub for Coord {
    type Output = (i8, i8);
    fn sub(self, other: Self) -> Self::Output { (self.row - other.row, self.col - other.col) }
}

impl fmt::Debug for Coord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Coord({})", self.to_algebraic())
    }
}


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
