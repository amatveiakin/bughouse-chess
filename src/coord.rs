use std::fmt;
use std::ops;

use itertools::Itertools;

use crate::force::Force;


pub const NUM_ROWS: u8 = 8;
pub const NUM_COLS: u8 = 8;


const fn const_char_sub(a: char, b: char) -> u8 {
    let a_idx = a as u32;
    let b_idx = b as u32;
    assert!(a_idx >= b_idx);
    let diff = a_idx - b_idx;
    assert!(diff <= u8::MAX as u32);
    diff as u8
}


// Row form a force's point of view
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SubjectiveRow {
    idx: u8,  // 0-based
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
    pub fn from_row(row: Row, force: Force) -> Self {
        match force {
            Force::White => Self::from_zero_based(row.idx),
            Force::Black => Self::from_zero_based(NUM_ROWS - row.idx - 1),
        }
    }
}


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Row {
    idx: u8,  // 0-based
}

impl Row {
    pub const fn from_zero_based(idx: u8) -> Self {
        assert!(idx < NUM_ROWS);
        Self { idx }
    }
    pub const fn from_algebraic(idx: char) -> Self {
        Self::from_zero_based(const_char_sub(idx, '1'))
    }
    pub const fn to_zero_based(self) -> u8 { self.idx }
    pub const fn to_algebraic(self) -> char { (self.idx + '1' as u8) as char }
    pub fn all() -> impl Iterator<Item = Self> + Clone {
        (0..NUM_ROWS).map(|idx| Self::from_zero_based(idx))
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
    idx: u8,  // 0-based
}

impl Col {
    pub const fn from_zero_based(idx: u8) -> Col {
        assert!(idx < NUM_COLS);
        Col { idx }
    }
    pub const fn from_algebraic(idx: char) -> Self {
        Self::from_zero_based(const_char_sub(idx, 'a'))
    }
    pub const fn to_zero_based(self) -> u8 { self.idx }
    pub const fn to_algebraic(self) -> char { (self.idx + 'a' as u8) as char }
    pub fn all() -> impl Iterator<Item = Self> + Clone {
        (0..NUM_COLS).map(|idx| Self::from_zero_based(idx))
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


#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Coord {
    pub row: Row,
    pub col: Col,
}

impl Coord {
    pub const fn new(row: Row, col: Col) -> Self {
        Self{ row, col }
    }
    pub fn from_algebraic(s: &str) -> Self {
        let chars: [char; 2] = s.chars().collect_vec().try_into().unwrap();
        Coord{ row: Row::from_algebraic(chars[1]), col: Col::from_algebraic(chars[0]) }
    }
    pub fn all() -> impl Iterator<Item = Coord> {
        Row::all().cartesian_product(Col::all()).map(|(row, col)| Coord{ row, col } )
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

impl fmt::Debug for Coord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Coord({}{})", self.col.to_algebraic(), self.row.to_algebraic())
    }
}


impl Row {
    #![allow(dead_code)]
    pub const _1: Row = Row::from_algebraic('1');
    pub const _2: Row = Row::from_algebraic('2');
    pub const _3: Row = Row::from_algebraic('3');
    pub const _4: Row = Row::from_algebraic('4');
    pub const _5: Row = Row::from_algebraic('5');
    pub const _6: Row = Row::from_algebraic('6');
    pub const _7: Row = Row::from_algebraic('7');
    pub const _8: Row = Row::from_algebraic('8');
}

impl Col {
    #![allow(dead_code)]
    pub const A: Col = Col::from_algebraic('a');
    pub const B: Col = Col::from_algebraic('b');
    pub const C: Col = Col::from_algebraic('c');
    pub const D: Col = Col::from_algebraic('d');
    pub const E: Col = Col::from_algebraic('e');
    pub const F: Col = Col::from_algebraic('f');
    pub const G: Col = Col::from_algebraic('g');
    pub const H: Col = Col::from_algebraic('h');
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
