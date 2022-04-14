use std::ops;

use console::Style;
use itertools::Itertools;

use crate::coord::{Row, Col, Coord, NUM_ROWS, NUM_COLS};
use crate::force::Force;
use crate::janitor::Janitor;
use crate::piece::{PieceKind, PieceOnBoard};


#[derive(Clone, Debug)]
pub struct Grid {
    data: [[Option<PieceOnBoard>; NUM_COLS as usize]; NUM_ROWS as usize],
}

impl Grid {
    pub fn new() -> Grid {
        Grid { data: Default::default() }
    }

    // Idea. A separate class GridView that allows to make only temporary changes.
    pub fn maybe_scoped_set(&mut self, change: Option<(Coord, Option<PieceOnBoard>)>)
        -> impl ops::DerefMut<Target = Grid> + '_
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

    pub fn scoped_set(&mut self, pos: Coord, piece: Option<PieceOnBoard>)
        -> impl ops::DerefMut<Target = Grid> + '_
    {
        let original_piece = self[pos];
        self[pos] = piece;
        Janitor::new(self, move |grid| grid[pos] = original_piece)
    }

    pub fn render_as_unicode(&self) -> String {
        let bg_colors = [
            Style::new().color256(233).on_color256(230),
            Style::new().color256(233).on_color256(222)
        ];
        let mut color_idx = 0;
        let mut ret = String::new();
        let mut rows = Row::all().collect_vec();
        rows.reverse();
        for row in rows.into_iter() {
            ret.push_str(&format_square(row.to_algebraic()));
            for col in Col::all() {
                ret.push_str(&bg_colors[color_idx].apply_to(
                    format_square(match self[Coord::new(row, col)] {
                        Some(piece) => to_unicode_char(piece.kind, piece.force),
                        None => ' ',
                    })
                ).to_string());
                color_idx = 1 - color_idx;
            }
            color_idx = 1 - color_idx;
            ret.push('\n');
        }
        ret.push_str(&format_square(' '));
        for col in Col::all() {
            ret.push_str(&format_square(col.to_algebraic()));
        }
        ret
    }
}

impl ops::Index<Coord> for Grid {
    type Output = Option<PieceOnBoard>;
    fn index(&self, pos: Coord) -> &Self::Output {
        &self.data[pos.row.to_zero_based() as usize][pos.col.to_zero_based() as usize]
    }
}

impl ops::IndexMut<Coord> for Grid {
    fn index_mut(&mut self, pos: Coord) -> &mut Self::Output {
        &mut self.data[pos.row.to_zero_based() as usize][pos.col.to_zero_based() as usize]
    }
}

fn format_square(ch: char) -> String {
    format!(" {} ", ch)
}

fn to_unicode_char(piece_kind: PieceKind, force: Force) -> char {
    use PieceKind::*;
    use Force::*;
    match (force, piece_kind) {
        (White, Pawn) => '♙',
        (White, Knight) => '♘',
        (White, Bishop) => '♗',
        (White, Rook) => '♖',
        (White, Queen) => '♕',
        (White, King) => '♔',
        (Black, Pawn) => '♟',
        (Black, Knight) => '♞',
        (Black, Bishop) => '♝',
        (Black, Rook) => '♜',
        (Black, Queen) => '♛',
        (Black, King) => '♚',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::{PieceKind, PieceOrigin};
    use crate::force::Force;

    fn make_piece(kind: PieceKind) -> PieceOnBoard {
        PieceOnBoard::new(kind, PieceOrigin::Innate, None, Force::White)
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
