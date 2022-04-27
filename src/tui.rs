// TODO: Use `crossterm` instead (fix: for some reason rendering reserve background
//   was more buggy with it).
use console::Style;
use itertools::Itertools;

use crate::coord::{Row, Col, Coord, NUM_COLS};
use crate::board::{Board, Reserve};
use crate::clock::{TimeMeasurement, GameInstant, Clock};
use crate::game::{ChessGame, BughouseBoard, BughouseGame};
use crate::grid::Grid;
use crate::force::Force;
use crate::piece::PieceKind;
use crate::player::Player;


const BOARD_WIDTH: usize = (NUM_COLS as usize + 2) * 3;

fn div_ceil(a: u128, b: u128) -> u128 { (a + b - 1) / b }

pub fn render_clock(clock: &Clock, force: Force, now: GameInstant) -> (String, usize) {
    // TODO: Support longer time controls (with hours)
    let is_active = clock.active_force() == Some(force);
    let millis = clock.time_left(force, now, TimeMeasurement::Approximate).as_millis();
    let sec = millis / 1000;
    let separator = |s| if !is_active || millis % 1000 >= 500 { s } else { " " };
    let mut clock_str = if sec >= 20 {
        format!("{:02}{}{:02}", sec / 60, separator(":"), sec % 60)
    } else {
        format!(" {:02}{}{}", sec, separator("."), div_ceil(millis, 100) % 10)
    };
    let clock_str_len = clock_str.len();
    if is_active {
        clock_str = Style::new().reverse().apply_to(clock_str).to_string();
    } else if millis == 0 {
        // Note. This will not apply to an active player, which is by design.
        // When the game is over, all clocks stop, so no player is active.
        // An active player can have zero time only in an online game client.
        // In this case we shouldn't paint the clock red (which means defeat)
        // before the server confirmed game result, because the game may have
        // ended earlier on the other board.
        clock_str = Style::new().on_red().apply_to(clock_str).to_string();
    }
    (clock_str, clock_str_len)
}

pub fn render_player(player: &Player) -> (String, usize) {
    (player.name.clone(), player.name.len())
}

pub fn render_header(clock: &Clock, player: &Player, force: Force, now: GameInstant, flip: bool) -> String {
    let (clock_str, clock_str_len) = render_clock(clock, force, now);
    let (player_str, player_str_len) = render_player(player);
    let space = String::from(' ').repeat(BOARD_WIDTH - clock_str_len - player_str_len);
    if flip {
        format!("{}{}{}\n", player_str, space, clock_str)
    } else {
        format!("{}{}{}\n", clock_str, space, player_str)
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

pub fn render_chess_game(game: &ChessGame, now: GameInstant) -> String {
    use Force::*;
    let board = game.board();
    format!(
        "{}\n{}\n{}",
        render_header(board.clock(), board.player(Black), Black, now, false),
        render_grid(board.grid()),
        render_header(board.clock(), board.player(White), White, now, false),
    )
}

pub fn render_bughouse_board(board: &Board, now: GameInstant, second_board: bool) -> String {
    use Force::*;
    format!(
        "{}\n{}{}{}\n{}",
        render_header(board.clock(), board.player(Black), Black, now, second_board),
        render_reserve(board.reserve(Black), Black),
        render_grid(board.grid()),
        render_reserve(board.reserve(White), White),
        render_header(board.clock(), board.player(White), White, now, second_board),
    )
}

pub fn render_bughouse_game(game: &BughouseGame, now: GameInstant) -> String {
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
