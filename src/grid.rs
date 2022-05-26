use std::fmt;
use std::ops;

use crate::coord::{Coord, NUM_ROWS, NUM_COLS};
use crate::janitor::Janitor;
use crate::piece::{PieceOrigin, PieceOnBoard, PieceForRepetitionDraw};
use serde::{Serialize, Deserialize};


pub type Grid = GenericGrid<PieceOnBoard>;
pub type GridForRepetitionDraw = GenericGrid<PieceForRepetitionDraw>;

// Improvement potential: Benchmark if it's better to change grid data type to a `Box`
//   (inline storage makes the object expensive to move which Rust does a lot).
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GenericGrid<T: Copy> {
    data: [[Option<T>; NUM_COLS as usize]; NUM_ROWS as usize],
}

impl<T: Copy> GenericGrid<T> {
    pub fn new() -> Self {
        GenericGrid { data: Default::default() }
    }

    pub fn map<U: Copy>(&self, f: impl Fn(T) -> U + Copy) -> GenericGrid<U> {
        GenericGrid {
            data: self.data.map(|inner| inner.map(|v| v.map(f))),
        }
    }

    // Idea. A separate class GridView that allows to make only temporary changes.
    pub fn maybe_scoped_set(&mut self, change: Option<(Coord, Option<T>)>)
        -> impl ops::DerefMut<Target = Self> + '_
    {
        let original = match change {
            None => None,
            Some((pos, new_piece)) => {
                let original_piece = self[pos];
                self[pos] = new_piece;
                Some((pos, original_piece))
            },
        };
        Janitor::new(self, move |grid| {
            if let Some((pos, original_piece)) = original {
                grid[pos] = original_piece;
            }
        })
    }

    pub fn scoped_set(&mut self, pos: Coord, piece: Option<T>)
        -> impl ops::DerefMut<Target = Self> + '_
    {
        let original_piece = self[pos];
        self[pos] = piece;
        Janitor::new(self, move |grid| grid[pos] = original_piece)
    }
}

impl<T: Copy> ops::Index<Coord> for GenericGrid<T> {
    type Output = Option<T>;
    fn index(&self, pos: Coord) -> &Self::Output {
        &self.data[pos.row.to_zero_based() as usize][pos.col.to_zero_based() as usize]
    }
}

impl<T: Copy> ops::IndexMut<Coord> for GenericGrid<T> {
    fn index_mut(&mut self, pos: Coord) -> &mut Self::Output {
        &mut self.data[pos.row.to_zero_based() as usize][pos.col.to_zero_based() as usize]
    }
}

fn debug_format_piece(piece: &PieceOnBoard) -> String {
    let mut s = format!("{:?}-{:?}", piece.force, piece.kind);
    if piece.origin != PieceOrigin::Innate {
        s.push_str(&format!("-{:?}", piece.origin));
    }
    s
}

impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Grid ")?;
        f.debug_list().entries(Coord::all().filter_map(|coord| {
            self[coord].map(|piece| {
                format!("{} => {}", coord.to_algebraic(), debug_format_piece(&piece))
            })
        })).finish()
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
    use crate::piece::{PieceKind, PieceOrigin};
    use crate::force::Force;

    fn make_piece(kind: PieceKind) -> PieceOnBoard {
        PieceOnBoard::new(kind, PieceOrigin::Innate, Force::White)
    }

    #[test]
    fn scoped_set() {
        let mut g = Grid::new();
        g[Coord::A1] = Some(make_piece(PieceKind::Queen));
        g[Coord::B2] = Some(make_piece(PieceKind::King));
        g[Coord::C3] = Some(make_piece(PieceKind::Rook));
        {
            let mut g = g.scoped_set(Coord::A1, Some(make_piece(PieceKind::Knight)));
            let mut g = g.scoped_set(Coord::A1, None);
            let g = g.scoped_set(Coord::C3, Some(make_piece(PieceKind::Bishop)));
            assert_eq!(g[Coord::A1], None);
            assert_eq!(g[Coord::B2], Some(make_piece(PieceKind::King)));
            assert_eq!(g[Coord::C3], Some(make_piece(PieceKind::Bishop)));
        }
        assert_eq!(g[Coord::A1], Some(make_piece(PieceKind::Queen)));
        assert_eq!(g[Coord::B2], Some(make_piece(PieceKind::King)));
        assert_eq!(g[Coord::C3], Some(make_piece(PieceKind::Rook)));
    }
}
