// Improvement potential: Allow to convert any position, not just starting.
// Improvement potential: Use classic castling notation if not Chess960.

use itertools::Itertools;
use strum::IntoEnumIterator;

use crate::grid::Grid;
use crate::board::Board;
use crate::coord::{Row, Col, Coord, NUM_ROWS, NUM_COLS};
use crate::force::Force;
use crate::piece::{CastleDirection, PieceKind, PieceOrigin, PieceOnBoard};


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

fn notation_to_piece(ch: char) -> Option<(PieceKind, Force)> {
    let kind = PieceKind::from_algebraic_char(ch.to_ascii_uppercase())?;
    let force = if ch.is_ascii_uppercase() { Force::White } else { Force::Black };
    Some((kind, force))
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

pub fn shredder_fen_to_starting_position(fen: &str) -> Result<Grid, String> {
    let (
        grid_notation,
        active_force_notation,
        _castling_notation,
        en_passant_target_notation,
        half_turn_clock,
        full_turn_index
    ) = fen.split_whitespace().collect_tuple().ok_or_else(||
        "Invalid Shredder-FEN format. Expected: piece_placement_data active_color\
        castling_availability en_passant_target_square halfmove_clock fullmove_number".to_owned()
    )?;
    let is_starting_pos =
        active_force_notation == "w" &&
        en_passant_target_notation == "-" &&
        half_turn_clock == "0" &&
        full_turn_index == "1";
    if !is_starting_pos {
        return Err("Only starting positions are supported".to_owned());
    }
    let rows = grid_notation.split('/').collect_vec();
    if rows.len() != usize::from(NUM_ROWS) {
        return Err(format!("Expected {NUM_ROWS} rows, found '{grid_notation}'"));
    }
    let mut grid = Grid::new();
    for (row_idx, row_fen) in rows.into_iter().rev().enumerate() {
        let mut col_idx = 0;
        for ch in row_fen.chars() {
            if let Some(skip) = ch.to_digit(10) {
                col_idx += skip;
            } else {
                let coord = Coord::new(
                    Row::from_zero_based(row_idx.try_into().unwrap()),
                    Col::from_zero_based(col_idx.try_into().unwrap())
                );
                let (piece_kind, force) = notation_to_piece(ch).ok_or_else(
                    || format!("Illegal piece notation: '{ch}'")
                )?;
                grid[coord] = Some(PieceOnBoard {
                    kind: piece_kind,
                    origin: PieceOrigin::Innate,
                    force,
                });
                col_idx += 1;
            }
        }
        if col_idx != u32::from(NUM_COLS) {
            return Err(format!("Expected {NUM_COLS} cols, found '{row_fen}'"));
        }
    }
    Ok(grid)
}
