// Improvement potential: Allow to convert any position, not just starting.
// Improvement potential: Use classic castling notation if not Chess960.

use enum_map::enum_map;
use itertools::Itertools;
use strum::IntoEnumIterator;

use crate::board::{Board, BoardCastlingRights};
use crate::coord::{BoardShape, Col, Coord, Row, SubjectiveRow};
use crate::force::Force;
use crate::grid::Grid;
use crate::piece::{
    piece_from_ascii, piece_to_ascii, CastleDirection, PieceId, PieceKind, PieceOnBoard,
    PieceOrigin,
};
use crate::rules::ChessRules;
use crate::starter::{assign_piece_ids, BoardSetup};
use crate::util::as_single_char;


fn force_to_fen(force: Force) -> char {
    match force {
        Force::White => 'w',
        Force::Black => 'b',
    }
}
fn force_from_fen(s: &str) -> Result<Force, String> {
    let ch = as_single_char(s).ok_or_else(|| format!("invalid force: {}", s))?;
    match ch {
        'w' => Ok(Force::White),
        'b' => Ok(Force::Black),
        _ => Err(format!("invalid force: {}", ch)),
    }
}

fn col_to_fen(board_shape: BoardShape, col: Col, force: Force) -> char {
    let s = col.to_algebraic(board_shape);
    match force {
        Force::White => s.to_ascii_uppercase(),
        Force::Black => s.to_ascii_lowercase(),
    }
}
fn col_from_fen(ch: char) -> Result<(Col, Force), String> {
    let col = Col::from_algebraic(ch.to_ascii_lowercase())
        .ok_or_else(|| format!("invalid file: {}", ch))?;
    let force = if ch.is_ascii_uppercase() {
        Force::White
    } else {
        Force::Black
    };
    Ok((col, force))
}

fn castling_rights_to_fen(
    board_shape: BoardShape, castling_rights: &BoardCastlingRights,
) -> String {
    let mut s = String::new();
    for force in Force::iter() {
        for dir in CastleDirection::iter() {
            if let Some(col) = castling_rights[force][dir] {
                s.push(col_to_fen(board_shape, col, force));
            }
        }
    }
    if s.is_empty() {
        "-".to_owned()
    } else {
        s
    }
}
fn castling_rights_from_fen(grid: &Grid, s: &str) -> Result<BoardCastlingRights, String> {
    let mut castling_rights = BoardCastlingRights::default();
    if s == "-" {
        return Ok(castling_rights);
    }
    let mut king_pos = enum_map! { _ => None };
    for pos in grid.shape().coords() {
        if let Some(piece) = grid[pos] {
            if piece.kind == PieceKind::King {
                let force: Force = piece.force.try_into().map_err(|_| "invalid king".to_owned())?;
                if king_pos[force].is_some() {
                    // Improvement potential. Allow parsing active Koedem games.
                    return Err("multiple kings".to_owned());
                }
                king_pos[force] = Some(pos);
            }
        }
    }
    for ch in s.chars() {
        let (col, force) = col_from_fen(ch)?;
        let king_pos = king_pos[force].ok_or_else(|| "missing king".to_owned())?;
        if king_pos.row != SubjectiveRow::first().to_row(grid.shape(), force) {
            return Err("cannot have castling rights when king is not in home row".to_owned());
        }
        let dir = if col < king_pos.col {
            CastleDirection::ASide
        } else {
            CastleDirection::HSide
        };
        castling_rights[force][dir] = Some(col);
    }
    Ok(castling_rights)
}

fn en_passant_target_to_fen(board_shape: BoardShape, en_passant_target: Option<Coord>) -> String {
    match en_passant_target {
        None => "-".to_owned(),
        Some(pos) => pos.to_algebraic(board_shape),
    }
}
fn en_passant_target_from_fen(s: &str) -> Result<Option<Coord>, String> {
    if s == "-" {
        Ok(None)
    } else {
        let pos =
            Coord::from_algebraic(s).ok_or_else(|| format!("invalid en passant target: {}", s))?;
        Ok(Some(pos))
    }
}

pub fn starting_position_to_shredder_fen(board: &Board) -> String {
    let half_turn_clock = 0; // since the last capture or pawn advance, for the fifty-move rule
    let full_turn_index = 1; // starts at 1 and is incremented after Black's move

    let grid = board.grid();
    let grid_notation = board
        .shape()
        .rows()
        .rev()
        .map(|row| {
            let mut row_notation = String::new();
            let mut empty_col_count: u8 = 0;
            for col in board.shape().cols() {
                if let Some(piece) = grid[Coord::new(row, col)] {
                    if empty_col_count > 0 {
                        row_notation.push_str(&empty_col_count.to_string());
                        empty_col_count = 0;
                    }
                    row_notation.push(piece_to_ascii(piece.kind, piece.force));
                    // Note. If this is extended to save arbitrary (not just starting) position,
                    // then we need to include "~" after promoted pieces.
                    // See https://bughousedb.com/Lieven_BPGN_Standard.txt, section 3.2.
                } else {
                    empty_col_count += 1;
                }
            }
            if empty_col_count > 0 {
                row_notation.push_str(&empty_col_count.to_string());
            }
            row_notation
        })
        .join("/");

    let active_force_notation = force_to_fen(board.active_force());
    let castling_notation = castling_rights_to_fen(board.shape(), board.castling_rights());
    let en_passant_target_notation =
        en_passant_target_to_fen(board.shape(), board.en_passant_target());

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

pub fn shredder_fen_to_starting_position(
    rules: &ChessRules, fen: &str,
) -> Result<BoardSetup, String> {
    let (
        grid_notation,
        active_force_notation,
        castling_notation,
        en_passant_target_notation,
        half_turn_clock,
        full_turn_index,
    ) = fen.split_whitespace().collect_tuple().unwrap();

    let mut grid = Grid::new(rules.board_shape());
    let rows = grid_notation.split('/').collect_vec();
    if rows.len() as u8 != rules.board_shape().num_rows {
        return Err(format!(
            "invalid FEN: has {} rows, expected {}",
            rows.len(),
            rules.board_shape().num_rows
        ));
    }
    for (row, row_notation) in rows.iter().rev().enumerate() {
        let row = row as i8;
        let mut col = 0;
        for ch in row_notation.chars() {
            if let Some(n) = ch.to_digit(10) {
                col += n as i8;
            } else if let Some((kind, force)) = piece_from_ascii(ch) {
                grid[Coord::new(Row::from_zero_based(row), Col::from_zero_based(col))] =
                    Some(PieceOnBoard {
                        id: PieceId::tmp(),
                        kind,
                        force,
                        origin: PieceOrigin::Innate, // because we only allow starting positions
                    });
                col += 1;
            }
        }
        if col as u8 != rules.board_shape().num_cols {
            return Err(format!(
                "invalid FEN: row {} has {} columns, expected {}",
                row,
                col,
                rules.board_shape().num_cols
            ));
        }
    }
    let mut next_piece_id = PieceId::new();
    assign_piece_ids(&mut grid, &mut next_piece_id);

    let active_force = force_from_fen(active_force_notation)?;
    let castling_rights = castling_rights_from_fen(&grid, castling_notation)?;
    let en_passant_target = en_passant_target_from_fen(en_passant_target_notation)?;
    let reserves = Default::default(); // TODO: save reserves to FEN
    let half_turn_clock = half_turn_clock
        .parse::<u32>()
        .map_err(|_| format!("invalid half-turn clock: {}", half_turn_clock))?;
    let full_turn_index = full_turn_index
        .parse::<u32>()
        .map_err(|_| format!("invalid full turn index: {}", full_turn_index))?;
    if half_turn_clock != 0 || full_turn_index != 1 {
        return Err("only starting positions are supported".to_owned());
    }

    Ok(BoardSetup {
        grid,
        next_piece_id,
        castling_rights,
        en_passant_target,
        reserves,
        active_force,
    })
}
