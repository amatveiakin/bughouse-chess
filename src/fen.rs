use itertools::Itertools;
use strum::IntoEnumIterator;

use crate::board::Board;
use crate::coord::{Row, Col, Coord};
use crate::force::Force;
use crate::piece::{CastleDirection, PieceKind};


fn force_notation(force: Force) -> char {
    match force {
        Force::White => 'w',
        Force::Black => 'b',
    }
}

fn piece_notation(kind: PieceKind, force: Force) -> char {
    let s = kind.to_full_algebraic();
    match force {
        Force::White => s.to_ascii_uppercase(),
        Force::Black => s.to_ascii_lowercase(),
    }
}

fn col_notation(col: Col, force: Force) -> char {
    let s = col.to_algebraic();
    match force {
        Force::White => s.to_ascii_uppercase(),
        Force::Black => s.to_ascii_lowercase(),
    }
}

fn make_castling_notation(board: &Board) -> String {
    let castling_rights = board.castling_rights();
    let mut s = String::new();
    for force in Force::iter() {
        for dir in CastleDirection::iter() {
            if let Some(col) = castling_rights[force][dir] {
                s.push(col_notation(col, force));
            }
        }
    }
    if s.is_empty() { "-".to_owned() } else { s }
}

// Improvement potential: Allow to convert any position, not just starting.
// Improvement potential: Allow to build a game from FEN.
// Improvement potential: Use classic castling notation if not Chess960.
pub fn starting_position_to_shredder_fen(board: &Board) -> String {
    let half_turn_clock = 0;  // since the last capture or pawn advance, for the fifty-move rule
    let full_turn_index = 1;  // starts at 1 and is incremented after Black's move

    let grid = board.grid();
    let grid_notation = Row::all().rev().map(|row| {
        let mut row_notation = String::new();
        let mut empty_col_count: u8 = 0;
        for col in Col::all() {
            if let Some(piece) = grid[Coord::new(row, col)] {
                if empty_col_count > 0 {
                    row_notation.push_str(&empty_col_count.to_string());
                    empty_col_count = 0;
                }
                row_notation.push(piece_notation(piece.kind, piece.force));
            } else {
                empty_col_count += 1;
            }
        }
        if empty_col_count > 0 {
            row_notation.push_str(&empty_col_count.to_string());
        }
        row_notation
    }).join("/");

    let active_force_notation = force_notation(board.active_force());
    let castling_notation = make_castling_notation(board);
    let en_passant_target_notation = match board.en_passant_target() {
        None => "-".to_owned(),
        Some(pos) => pos.to_algebraic(),
    };

    format!(
        "{} {} {} {} {} {}",
        grid_notation,
        active_force_notation,
        castling_notation,
        en_passant_target_notation,
        half_turn_clock,
        full_turn_index
    )
}
