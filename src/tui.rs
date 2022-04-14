// TODO: Use `crossterm` instead (fix: for some reason rendering reserve background
//   was more buggy with it).
use console::Style;
use itertools::Itertools;

use crate::coord::{Row, Col, Coord, NUM_COLS};
use crate::chess::{Board, Reserve, ChessGame, BughouseGame};
use crate::grid::Grid;
use crate::force::Force;
use crate::piece::PieceKind;


pub fn render_chess_game(game: &ChessGame) -> String {
    render_grid(game.board().grid())
}

pub fn render_reserve(reserve: &Reserve, force: Force, width: usize) -> String {
    let mut stacks = Vec::new();
    for (piece_kind, &amount) in reserve.iter() {
        if amount > 0 {
            stacks.push(String::from(to_unicode_char(piece_kind, force)).repeat(amount.into()));
        }
    }
    format!(
        "{1:^0$}\n",
         width,
         Style::new().color256(233).on_color256(195).apply_to(stacks.iter().join(" "))
    )
}

pub fn render_bughouse_board(board: &Board) -> String {
    let width: usize = ((NUM_COLS + 2) * 3).into();
    format!(
        "{}{}{}",
        render_reserve(board.reserve(Force::Black), Force::Black, width),
        render_grid(board.grid()),
        render_reserve(board.reserve(Force::White), Force::White, width),
    )
}

pub fn render_bughouse_game(game: &BughouseGame) -> String {
    let board1 = render_bughouse_board(game.board(0));
    let board2 = render_bughouse_board(game.board(1));
    board1.lines().zip(board2.lines().rev()).map(|(line1, line2)| {
        format!("{}      {}", line1, line2)
    }).join("\n")
}

pub fn render_grid(grid: &Grid) -> String {
    let colors = [
        Style::new().color256(233).on_color256(230),
        Style::new().color256(233).on_color256(222)
    ];

    let mut col_names = String::new();
    col_names.push_str(&format_square(' '));
    for col in Col::all() {
        col_names.push_str(&format_square(col.to_algebraic()));
    }
    col_names.push_str(&format_square(' '));
    col_names.push('\n');

    let mut color_idx = 0;
    let mut ret = String::new();
    ret.push_str(&col_names);
    let mut rows = Row::all().collect_vec();
    rows.reverse();
    for row in rows.into_iter() {
        ret.push_str(&format_square(row.to_algebraic()));
        for col in Col::all() {
            ret.push_str(&colors[color_idx].apply_to(
                format_square(match grid[Coord::new(row, col)] {
                    Some(piece) => to_unicode_char(piece.kind, piece.force),
                    None => ' ',
                })
            ).to_string());
            color_idx = 1 - color_idx;
        }
        ret.push_str(&format_square(row.to_algebraic()));
        color_idx = 1 - color_idx;
        ret.push('\n');
    }
    ret.push_str(&col_names);
    ret
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
