// Improvement potential. Use `crossterm` instead (fix: for some reason rendering
//   reserve background was more buggy with it).

use console::Style;
use itertools::Itertools;

use crate::bughouse_prelude::*;


fn render_clock(showing: ClockShowing) -> (String, usize) {
    let mut clock_str = showing.ui_string();
    let clock_str_len = clock_str.len();
    if showing.out_of_time {
        clock_str = Style::new().on_red().apply_to(clock_str).to_string();
    } else if showing.is_active {
        clock_str = Style::new().reverse().apply_to(clock_str).to_string();
    }
    (clock_str, clock_str_len)
}

fn render_player(player_name: &str) -> (String, usize) {
    (player_name.to_owned(), player_name.len())
}

fn render_header(
    clock: &Clock, player_name: &str, force: Force, now: GameInstant, view_board: DisplayBoard,
    board_width: usize,
) -> String {
    let (clock_str, clock_str_len) = render_clock(clock.showing_for(force, now));
    let (player_str, player_str_len) = render_player(player_name);
    let space = String::from(' ').repeat(board_width - clock_str_len - player_str_len);
    match view_board {
        DisplayBoard::Primary => format!("{}{}{}\n", clock_str, space, player_str),
        DisplayBoard::Secondary => format!("{}{}{}\n", player_str, space, clock_str),
    }
}

fn render_reserve(reserve: &Reserve, force: Force, board_width: usize) -> String {
    let mut stacks = Vec::new();
    for (piece_kind, &amount) in reserve.iter() {
        if amount > 0 {
            stacks.push(
                String::from(piece_to_pictogram(piece_kind, force.into())).repeat(amount.into()),
            );
        }
    }
    format!(
        "{1:^0$}\n",
        board_width,
        Style::new().color256(233).on_color256(194).apply_to(stacks.iter().join(" "))
    )
}

fn render_bughouse_board(
    board: &Board, now: GameInstant, view_board: DisplayBoard, perspective: Perspective,
) -> String {
    use self::Force::*;
    let orientation = get_board_orientation(view_board, perspective);
    let board_width = (board.shape().num_cols as usize + 2) * 3;
    format!(
        "{}\n{}{}{}\n{}",
        render_header(board.clock(), board.player_name(Black), Black, now, view_board, board_width),
        render_reserve(board.reserve(Black), Black, board_width),
        render_grid(board.grid(), orientation),
        render_reserve(board.reserve(White), White, board_width),
        render_header(board.clock(), board.player_name(White), White, now, view_board, board_width),
    )
}

pub fn render_bughouse_game(
    game: &BughouseGame, my_id: BughouseParticipant, now: GameInstant,
) -> String {
    use DisplayBoard::*;
    let perspective = Perspective::for_participant(my_id);
    let primary_idx = get_board_index(Primary, perspective);
    let secondary_idx = get_board_index(Secondary, perspective);
    let board_primary = render_bughouse_board(game.board(primary_idx), now, Primary, perspective);
    let board_secondary =
        render_bughouse_board(game.board(secondary_idx), now, Secondary, perspective);
    board_primary
        .lines()
        .zip(board_secondary.lines())
        .map(|(l1, l2)| format!("{}      {}", l1, l2))
        .join("\n")
}

fn render_grid(grid: &Grid, orientation: BoardOrientation) -> String {
    let colors = [
        Style::new().color256(233).on_color256(222),
        Style::new().color256(233).on_color256(230),
    ];
    let board_shape = grid.shape();
    let mut ret = String::new();
    for y in (-1)..=(board_shape.num_cols as i32) {
        for x in (-1)..=(board_shape.num_rows as i32) {
            let row_header = x < 0 || x >= board_shape.num_cols.into();
            let col_header = y < 0 || y >= board_shape.num_rows.into();
            let square = match (row_header, col_header) {
                (true, true) => format_square(' '),
                (true, false) => format_square(
                    from_display_row(y.try_into().unwrap(), board_shape, orientation)
                        .unwrap()
                        .to_algebraic(board_shape),
                ),
                (false, true) => format_square(
                    from_display_col(x.try_into().unwrap(), board_shape, orientation)
                        .unwrap()
                        .to_algebraic(board_shape),
                ),
                (false, false) => {
                    let coord = from_display_coord(
                        DisplayCoord {
                            x: x.try_into().unwrap(),
                            y: y.try_into().unwrap(),
                        },
                        board_shape,
                        orientation,
                    )
                    .unwrap();
                    let color_idx = (coord.row.to_zero_based() + coord.col.to_zero_based()) % 2;
                    colors[usize::try_from(color_idx).unwrap()]
                        .apply_to(format_square(match grid[coord] {
                            Some(piece) => piece_to_pictogram(piece.kind, piece.force),
                            None => ' ',
                        }))
                        .to_string()
                }
            };
            ret.push_str(&square);
        }
        ret.push('\n');
    }
    ret
}

fn format_square(ch: char) -> String { format!(" {} ", ch) }
