// Improvement potential: Allow to convert any position, not just starting.
// Improvement potential: Use classic castling notation if not Chess960.

use enum_map::enum_map;
use itertools::Itertools;
use strum::IntoEnumIterator;

use crate::board::{Board, BoardCastlingRights};
use crate::coord::{BoardShape, Col, Coord, Row, SubjectiveRow};
use crate::force::Force;
use crate::grid::Grid;
use crate::once_cell_regex;
use crate::piece::{
    CastleDirection, PieceId, PieceKind, PieceOnBoard, PieceOrigin, piece_from_ascii,
    piece_to_ascii,
};
use crate::rules::ChessRules;
use crate::starter::{BoardSetup, assign_piece_ids};
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
    if s.is_empty() { "-".to_owned() } else { s }
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

// Differences from classic FEN notation:
//   - Castling uses files rather than king-side/queen-side notation (Shredder-FEN).
//   - A tilde is added after promoted pieces (BPGN standard)
//   - Halfmove clock is always set to 0 and ignored when reading: we don't use the fifty-move rule.
//   - If not empty, reserve is listed in square brackets after the position (like Fairy-Stockfish).
pub fn board_to_shredder_fen(board: &Board) -> String {
    let half_turn_clock = 0; // we don't use the fifty-move rule
    let full_turn_index = board.full_turn_index();

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
                    match piece.origin {
                        PieceOrigin::Innate => {}
                        PieceOrigin::Promoted => {
                            // Include "~" after promoted pieces:
                            // https://bughousedb.com/Lieven_BPGN_Standard.txt, section 3.2.
                            row_notation.push('~');
                        }
                        PieceOrigin::Combined(_) => {
                            // TODO: Deal with promoted combined pieces.
                        }
                        PieceOrigin::Dropped => {}
                    }
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

    let mut reserve_notation = Force::iter()
        .map(|force| {
            board
                .reserve(force)
                .iter()
                .filter(|(piece_kind, _)| !piece_kind.is_neutral()) // exclude the duck
                .map(|(piece_kind, &count)| {
                    String::from(piece_to_ascii(piece_kind, force.into())).repeat(count.into())
                })
                .join("")
        })
        .join("");
    if !reserve_notation.is_empty() {
        reserve_notation = format!("[{}]", reserve_notation);
    }

    let active_force_notation = force_to_fen(board.active_force());
    let castling_notation = castling_rights_to_fen(board.shape(), board.castling_rights());
    let en_passant_target_notation =
        en_passant_target_to_fen(board.shape(), board.en_passant_target());

    format!(
        "{}{} {} {} {} {} {}",
        grid_notation,
        reserve_notation,
        active_force_notation,
        castling_notation,
        en_passant_target_notation,
        half_turn_clock,
        full_turn_index
    )
}

pub fn shredder_fen_to_board(rules: &ChessRules, fen: &str) -> Result<BoardSetup, String> {
    let reserve_re = once_cell_regex!(r"^(.*)\[(.*)\]$");

    let (
        mut grid_notation,
        active_force_notation,
        castling_notation,
        en_passant_target_notation,
        half_turn_clock,
        full_turn_index,
    ) = fen
        .split_whitespace()
        .collect_tuple()
        .ok_or_else(|| format!("invalid FEN: {fen}"))?;

    let mut reserves = enum_map! { _ => enum_map!{ _ => 0 } };
    if let Some(cap) = reserve_re.captures(grid_notation) {
        grid_notation = &grid_notation[cap.get(1).unwrap().range()];
        let reserve_notation = cap.get(2).unwrap().as_str();
        for piece in reserve_notation.chars() {
            if let Some((kind, piece_force)) = piece_from_ascii(piece) {
                let force = piece_force.try_into().map_err(|_| {
                    format!("invalid FEN: unexpected neutral reserve piece: {piece}")
                })?;
                reserves[force][kind] += 1;
            } else {
                return Err(format!("invalid FEN: unknown reserve piece: {piece}"));
            }
        }
    }

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
        let mut row_iter = row_notation.chars().peekable();
        while let Some(ch) = row_iter.next() {
            if let Some(n) = ch.to_digit(10) {
                col += n as i8;
            } else if let Some((kind, force)) = piece_from_ascii(ch) {
                let mut piece = PieceOnBoard {
                    id: PieceId::tmp(),
                    kind,
                    force,
                    // TODO: Fix `PieceOrigin::Combined`.
                    // TODO: How bad is it that we misidentify `Dropped` pieces as `Innate`?
                    origin: PieceOrigin::Innate,
                };
                if row_iter.peek() == Some(&'~') {
                    piece.origin = PieceOrigin::Promoted;
                    row_iter.next();
                }
                let coord = Coord::new(Row::from_zero_based(row), Col::from_zero_based(col));
                grid[coord] = Some(piece);
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
    // Ignore `half_turn_clock`: we don't use the fifty-move rule.
    let _ = half_turn_clock
        .parse::<u32>()
        .map_err(|_| format!("invalid half-turn clock: {}", half_turn_clock))?;
    let full_turn_index = full_turn_index
        .parse::<u32>()
        .map_err(|_| format!("invalid full turn index: {}", full_turn_index))?;

    Ok(BoardSetup {
        grid,
        next_piece_id,
        castling_rights,
        en_passant_target,
        reserves,
        full_turn_index,
        active_force,
    })
}


#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::game::{BughouseBoard, BughouseGame};
    use crate::role::Role;
    use crate::rules::{MatchRules, Rules};
    use crate::test_util::{replay_bughouse_log, sample_bughouse_players};

    fn comparable(setup: BoardSetup) -> BoardSetup {
        BoardSetup {
            grid: setup.grid.map(|piece| {
                // Improvement potential: The fact that we have to workaround origin is not great.
                // Origin is important information that affects the game flow. We should either
                // preserve the data to FEN or live without it.
                use PieceOrigin::*;
                let origin = match piece.origin {
                    Innate | Combined(_) | Dropped => Innate,
                    Promoted => Promoted,
                };
                PieceOnBoard { origin, id: PieceId::tmp(), ..piece }
            }),
            next_piece_id: PieceId::tmp(),
            ..setup
        }
    }

    #[test]
    fn promoted_pieces() {
        let rules = Rules {
            match_rules: MatchRules::unrated_public(),
            chess_rules: ChessRules::bughouse_international5(),
        };
        let mut game =
            BughouseGame::new(rules.clone(), Role::ServerOrStandalone, &sample_bughouse_players());
        replay_bughouse_log(
            &mut game,
            "1A.a4 1a.h5 2A.a5 2a.h4 3A.a6 3a.h3 4A.xb7 4a.xg2 5A.xc8/Q 5a.xh1/N",
            Duration::from_millis(100),
        )
        .unwrap();
        let board = game.board(BughouseBoard::A);
        let fen = board_to_shredder_fen(board);
        // Note the tildes ("~") after promoted pieces.
        assert_eq!(fen, "rnQ~qkbnr/p1ppppp1/8/8/8/8/1PPPPP1P/RNBQKBNn~ w Aah - 0 6");
        let parsed_board = shredder_fen_to_board(&rules.chess_rules, &fen).unwrap();
        assert_eq!(comparable(parsed_board), comparable(board.clone().into()));
    }

    #[test]
    fn reserves() {
        let rules = Rules {
            match_rules: MatchRules::unrated_public(),
            chess_rules: ChessRules::bughouse_international5(),
        };
        let mut game =
            BughouseGame::new(rules.clone(), Role::ServerOrStandalone, &sample_bughouse_players());
        replay_bughouse_log(
            &mut game,
            "
                1A.e4 1B.e4 1b.e6 1a.d5 2B.d4 2A.xd5 2a.e6 3A.xe6 2b.Be7 3a.Bxe6 3B.Nf3 4A.Nf3
                3b.Nf6 4a.Be7 5A.d4 5a.Nf6 4B.Nc3 4b.P@b4 6A.Bg5 6a.h6 7A.Bxf6 7a.Bxf6 8A.Be2
                5B.Bd2 8a.Nc6 5b.xc3 6B.Bxc3 6b.Nxe4 7B.Qe2 7b.Nxc3 8B.xc3 9A.P@e5 8b.O-O 9a.Be7
            ",
            Duration::from_millis(100),
        )
        .unwrap();
        let board = game.board(BughouseBoard::A);
        let fen = board_to_shredder_fen(board);
        // Note the reserve pieces listed in square brackets.
        assert_eq!(fen, "r2qk2r/ppp1bpp1/2n1b2p/4P3/3P4/5N2/PPP1BPPP/RN1QK2R[NBpn] w AHah - 0 10");
        let parsed_board = shredder_fen_to_board(&rules.chess_rules, &fen).unwrap();
        assert_eq!(comparable(parsed_board), comparable(board.clone().into()));
    }
}
