// Improvement potential. Use `crossterm` instead (fix: for some reason rendering
//   reserve background was more buggy with it).

use bughouse_chess::*;
use console::Style;
use itertools::Itertools;


const BOARD_WIDTH: usize = (NUM_COLS as usize + 2) * 3;

fn render_clock(clock: &Clock, force: Force, now: GameInstant) -> (String, usize) {
    // Improvement potential: Support longer time controls (with hours).
    let ClockShowing {
        is_active,
        show_separator,
        out_of_time,
        time_breakdown,
    } = clock.showing_for(force, now);
    let separator = |s| if show_separator { s } else { " " };
    let mut clock_str = match time_breakdown {
        TimeBreakdown::NormalTime { minutes, seconds } => {
            format!("{:02}{}{:02}", minutes, separator(":"), seconds)
        }
        TimeBreakdown::LowTime { seconds, deciseconds } => {
            format!(" {:02}{}{}", seconds, separator("."), deciseconds)
        }
    };
    let clock_str_len = clock_str.len();
    if out_of_time {
        clock_str = Style::new().on_red().apply_to(clock_str).to_string();
    } else if is_active {
        clock_str = Style::new().reverse().apply_to(clock_str).to_string();
    }
    (clock_str, clock_str_len)
}

fn render_player(player_name: &str) -> (String, usize) {
    (player_name.to_owned(), player_name.len())
}

fn render_header(
    clock: &Clock, player_name: &str, force: Force, now: GameInstant, view_board: DisplayBoard,
) -> String {
    let (clock_str, clock_str_len) = render_clock(clock, force, now);
    let (player_str, player_str_len) = render_player(player_name);
    let space = String::from(' ').repeat(BOARD_WIDTH - clock_str_len - player_str_len);
    match view_board {
        DisplayBoard::Primary => format!("{}{}{}\n", clock_str, space, player_str),
        DisplayBoard::Secondary => format!("{}{}{}\n", player_str, space, clock_str),
    }
}

fn render_reserve(reserve: &Reserve, force: Force) -> String {
    let mut stacks = Vec::new();
    for (piece_kind, &amount) in reserve.iter() {
        if amount > 0 {
            stacks.push(String::from(piece_to_pictogram(piece_kind, force)).repeat(amount.into()));
        }
    }
    format!(
        "{1:^0$}\n",
        BOARD_WIDTH,
        Style::new().color256(233).on_color256(194).apply_to(stacks.iter().join(" "))
    )
}

fn render_bughouse_board(
    board: &Board, now: GameInstant, view_board: DisplayBoard, perspective: Perspective,
) -> String {
    use self::Force::*;
    let orientation = get_board_orientation(view_board, perspective);
    format!(
        "{}\n{}{}{}\n{}",
        render_header(board.clock(), board.player_name(Black), Black, now, view_board),
        render_reserve(board.reserve(Black), Black),
        render_grid(board.grid(), orientation),
        render_reserve(board.reserve(White), White),
        render_header(board.clock(), board.player_name(White), White, now, view_board),
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
    let mut ret = String::new();
    for y in (-1)..=(NUM_COLS as i32) {
        for x in (-1)..=(NUM_ROWS as i32) {
            let row_header = x < 0 || x >= NUM_COLS.into();
            let col_header = y < 0 || y >= NUM_ROWS.into();
            let square = match (row_header, col_header) {
                (true, true) => format_square(' '),
                (true, false) => format_square(
                    from_display_row(y.try_into().unwrap(), orientation).unwrap().to_algebraic(),
                ),
                (false, true) => format_square(
                    from_display_col(x.try_into().unwrap(), orientation).unwrap().to_algebraic(),
                ),
                (false, false) => {
                    let coord = from_display_coord(
                        DisplayCoord {
                            x: x.try_into().unwrap(),
                            y: y.try_into().unwrap(),
                        },
                        orientation,
                    )
                    .unwrap();
                    let color_idx = (coord.row.to_zero_based() + coord.col.to_zero_based()) % 2;
                    colors[usize::from(color_idx)]
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
