use std::time::Instant;

// TODO: Use `crossterm` instead (fix: for some reason rendering reserve background
//   was more buggy with it).
use console::Style;
use itertools::Itertools;

use crate::coord::{Row, Col, Coord, NUM_COLS};
use crate::chess::{Board, Reserve, ChessGame, BughouseBoard, BughouseGame};
use crate::clock::Clock;
use crate::grid::Grid;
use crate::force::Force;
use crate::piece::PieceKind;


const BOARD_WIDTH: usize = (NUM_COLS as usize + 2) * 3;

fn div_ceil(a: u128, b: u128) -> u128 { (a + b - 1) / b }

pub fn render_clock(clock: &Clock, force: Force, now: Instant, align_right: bool) -> String {
    // TODO: Support longer time controls (with hours)
    let is_active = clock.active_force() == Some(force);
    let millis = clock.time_left(force, now).as_millis();
    let sec = millis / 1000;
    let separator = |s| if !is_active || millis % 1000 >= 500 { s } else { " " };
    let mut time_str = if sec >= 20 {
        format!("{:02}{}{:02}", sec / 60, separator(":"), sec % 60)
    } else {
        format!(" {:02}{}{}", sec, separator("."), div_ceil(millis, 100) % 10)
    };
    let space = String::from(' ').repeat(BOARD_WIDTH - time_str.len());
    if is_active {
        time_str = Style::new().reverse().apply_to(time_str).to_string();
    } else if millis == 0 {
        time_str = Style::new().on_red().apply_to(time_str).to_string();
    }
    if align_right {
        format!("{}{}\n", space, time_str)
    } else {
        format!("{}{}\n", time_str, space)
    }
}

pub fn render_reserve(reserve: &Reserve, force: Force) -> String {
    let mut stacks = Vec::new();
    for (piece_kind, &amount) in reserve.iter() {
        if amount > 0 {
            stacks.push(String::from(to_unicode_char(piece_kind, force)).repeat(amount.into()));
        }
    }
    format!(
        "{1:^0$}\n",
        BOARD_WIDTH,
         Style::new().color256(233).on_color256(194).apply_to(stacks.iter().join(" "))
    )
}

pub fn render_chess_game(game: &ChessGame, now: Instant) -> String {
    let board = game.board();
    format!(
        "{}\n{}\n{}",
        render_clock(board.clock(), Force::Black, now, false),
        render_grid(board.grid()),
        render_clock(board.clock(), Force::White, now, false),
    )
}

pub fn render_bughouse_board(board: &Board, now: Instant, second_board: bool) -> String {
    format!(
        "{}\n{}{}{}\n{}",
        render_clock(board.clock(), Force::Black, now, second_board),
        render_reserve(board.reserve(Force::Black), Force::Black),
        render_grid(board.grid()),
        render_reserve(board.reserve(Force::White), Force::White),
        render_clock(board.clock(), Force::White, now, second_board),
    )
}

pub fn render_bughouse_game(game: &BughouseGame, now: Instant) -> String {
    let board1 = render_bughouse_board(game.board(BughouseBoard::A), now, false);
    let board2 = render_bughouse_board(game.board(BughouseBoard::B), now, true);
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
