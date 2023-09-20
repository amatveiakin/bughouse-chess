use std::{fmt, ops};

use ndarray::{Array, Array2};
use serde::{Deserialize, Serialize};

use crate::coord::{BoardShape, Col, Coord, Row};
use crate::janitor::Janitor;
use crate::piece::{PieceForRepetitionDraw, PieceOnBoard, PieceOrigin};


pub type Grid = GenericGrid<PieceOnBoard>;
pub type GridForRepetitionDraw = GenericGrid<PieceForRepetitionDraw>;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GridItem<T> {
    Piece(T),
    Empty,
    OutOfBounds,
}

impl<T> GridItem<T> {
    pub fn is_free(&self) -> bool { matches!(self, GridItem::Empty) }
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GenericGrid<T: Clone> {
    data: Array2<Option<T>>,
}

impl<T: Clone> GenericGrid<T> {
    pub fn new(board_shape: BoardShape) -> Self {
        GenericGrid {
            data: Array::from_elem(
                (board_shape.num_rows as usize, board_shape.num_cols as usize),
                None,
            ),
        }
    }

    pub fn shape(&self) -> BoardShape {
        BoardShape {
            num_rows: self.data.shape()[0] as u8,
            num_cols: self.data.shape()[1] as u8,
        }
    }

    pub fn contains_row(&self, row: Row) -> bool { self.shape().contains_row(row) }
    pub fn contains_col(&self, col: Col) -> bool { self.shape().contains_col(col) }
    pub fn contains_coord(&self, coord: Coord) -> bool { self.shape().contains_coord(coord) }

    pub fn get(&self, pos: Coord) -> GridItem<&T> {
        match self.data.get(coord_to_index(pos)) {
            None => GridItem::OutOfBounds,
            Some(None) => GridItem::Empty,
            Some(Some(v)) => GridItem::Piece(v),
        }
    }

    pub fn map<U: Clone>(&self, f: impl FnMut(T) -> U + Copy) -> GenericGrid<U> {
        GenericGrid { data: self.data.mapv(|v| v.map(f)) }
    }

    // Idea. A separate class GridView that allows to make only temporary changes.
    pub fn maybe_scoped_set(
        &mut self, change: Option<(Coord, Option<T>)>,
    ) -> impl ops::DerefMut<Target = Self> + '_ {
        let original = match change {
            None => None,
            Some((pos, new_piece)) => {
                let original_piece = self[pos].take();
                self[pos] = new_piece;
                Some((pos, original_piece))
            }
        };
        Janitor::new(self, move |grid| {
            if let Some((pos, ref original_piece)) = original {
                grid[pos] = original_piece.clone();
            }
        })
    }

    pub fn scoped_set(
        &mut self, pos: Coord, piece: Option<T>,
    ) -> impl ops::DerefMut<Target = Self> + '_ {
        let original_piece = self[pos].take();
        self[pos] = piece;
        Janitor::new(self, move |grid| grid[pos] = original_piece.clone())
    }
}

impl<T: Clone> ops::Index<Coord> for GenericGrid<T> {
    type Output = Option<T>;
    #[track_caller]
    fn index(&self, pos: Coord) -> &Self::Output {
        let shape = self.shape();
        self.data
            .get(coord_to_index(pos))
            .unwrap_or_else(|| panic!("{}", out_of_bound_message(pos, shape)))
    }
}

impl<T: Clone> ops::IndexMut<Coord> for GenericGrid<T> {
    #[track_caller]
    fn index_mut(&mut self, pos: Coord) -> &mut Self::Output {
        let shape = self.shape();
        self.data
            .get_mut(coord_to_index(pos))
            .unwrap_or_else(|| panic!("{}", out_of_bound_message(pos, shape)))
    }
}

fn coord_to_index(pos: Coord) -> [usize; 2] {
    [
        pos.row.to_zero_based() as usize,
        pos.col.to_zero_based() as usize,
    ]
}

fn out_of_bound_message(pos: Coord, board_shape: BoardShape) -> String {
    format!(
        "Coord ({}, {}) is out of bound for {}x{} board",
        pos.row.to_zero_based(),
        pos.col.to_zero_based(),
        board_shape.num_rows,
        board_shape.num_cols
    )
}

fn debug_format_piece(piece: &PieceOnBoard) -> String {
    let mut s = format!("[{}]-{:?}-{:?}", piece.id.0, piece.force, piece.kind);
    if piece.origin != PieceOrigin::Innate {
        s.push_str(&format!("-{:?}", piece.origin));
    }
    s
}

impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Grid ")?;
        f.debug_map()
            .entries(self.shape().coords().filter_map(|coord| {
                self[coord]
                    .map(|piece| (coord.to_algebraic(self.shape()), debug_format_piece(&piece)))
            }))
            .finish()
    }
}

impl fmt::Debug for GridForRepetitionDraw {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "GridForRepetitionDraw {:?}", self.data)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::{PieceForce, PieceId, PieceKind, PieceOrigin};

    #[test]
    fn scoped_set() {
        let mut piece_id = PieceId::new();
        let mut make_piece =
            |kind| PieceOnBoard::new(piece_id.inc(), kind, PieceOrigin::Innate, PieceForce::White);
        let mut g = Grid::new(BoardShape { num_rows: 8, num_cols: 8 });
        g[Coord::A1] = Some(make_piece(PieceKind::Queen));
        g[Coord::B2] = Some(make_piece(PieceKind::King));
        g[Coord::C3] = Some(make_piece(PieceKind::Rook));
        {
            let mut g = g.scoped_set(Coord::A1, Some(make_piece(PieceKind::Knight)));
            let mut g = g.scoped_set(Coord::A1, None);
            let g = g.scoped_set(Coord::C3, Some(make_piece(PieceKind::Bishop)));
            assert_eq!(g[Coord::A1], None);
            assert_eq!(g[Coord::B2].unwrap().kind, PieceKind::King);
            assert_eq!(g[Coord::C3].unwrap().kind, PieceKind::Bishop);
        }
        assert_eq!(g[Coord::A1].unwrap().kind, PieceKind::Queen);
        assert_eq!(g[Coord::B2].unwrap().kind, PieceKind::King);
        assert_eq!(g[Coord::C3].unwrap().kind, PieceKind::Rook);
    }
}
