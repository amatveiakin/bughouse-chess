// Improvement potential. Use `crossterm` instead (fix: for some reason rendering
//   reserve background was more buggy with it).

use console::Style;
use itertools::Itertools;

use bughouse_chess::*;
use bughouse_chess::util::div_ceil_u128;


const BOARD_WIDTH: usize = (NUM_COLS as usize + 2) * 3;

fn render_clock(clock: &Clock, force: Force, now: GameInstant) -> (String, usize) {
    // Improvement potential: Support longer time controls (with hours).
    let is_active = clock.active_force() == Some(force);
    let millis = clock.time_left(force, now).as_millis();
    let sec = millis / 1000;
    let separator = |s| if !is_active || millis % 1000 >= 500 { s } else { " " };
    let mut clock_str = if sec >= 20 {
        format!("{:02}{}{:02}", sec / 60, separator(":"), sec % 60)
    } else {
        format!(" {:02}{}{}", sec, separator("."), div_ceil_u128(millis, 100) % 10)
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

fn render_player(player_name: &str) -> (String, usize) {
    (player_name.to_owned(), player_name.len())
}

fn render_header(
    clock: &Clock, player_name: &str, force: Force, now: GameInstant, view_board: DisplayBoard
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
            stacks.push(String::from(to_unicode_char(piece_kind, force)).repeat(amount.into()));
        }
    }
    format!(
        "{1:^0$}\n",
        BOARD_WIDTH,
         Style::new().color256(233).on_color256(194).apply_to(stacks.iter().join(" "))
    )
}

fn render_bughouse_board(
    board: &Board, now: GameInstant, view_board: DisplayBoard, perspective: Perspective
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

pub fn render_bughouse_game(game: &BughouseGame, my_id: BughouseParticipantId, now: GameInstant) -> String {
    use DisplayBoard::*;
    let perspective = Perspective::for_force(my_id.visual_force());
    let primary_idx = get_board_index(Primary, my_id);
    let secondary_idx = get_board_index(Secondary, my_id);
    let board_primary = render_bughouse_board(game.board(primary_idx), now, Primary, perspective);
    let board_secondary = render_bughouse_board(game.board(secondary_idx), now, Secondary, perspective);
    board_primary.lines().zip(board_secondary.lines()).map(
        |(l1, l2)| format!("{}      {}", l1, l2)
    ).join("\n")
}

fn render_grid(grid: &Grid, orientation: BoardOrientation) -> String {
    let colors = [
        Style::new().color256(233).on_color256(222),
        Style::new().color256(233).on_color256(230),
    ];
    let mut ret = String::new();
    for y in (-1) ..= (NUM_COLS as i32) {
        for x in (-1) ..= (NUM_ROWS as i32) {
            let row_header = x < 0 || x >= NUM_COLS.into();
            let col_header = y < 0 || y >= NUM_ROWS.into();
            let square = match (row_header, col_header) {
                (true, true) => format_square(' '),
                (true, false) => format_square(from_display_row(y.try_into().unwrap(), orientation).to_algebraic()),
                (false, true) => format_square(from_display_col(x.try_into().unwrap(), orientation).to_algebraic()),
                (false, false) => {
                    let coord = from_display_coord(DisplayCoord {
                        x: x.try_into().unwrap(),
                        y: y.try_into().unwrap()
                    }, orientation);
                    let color_idx = (coord.row.to_zero_based() + coord.col.to_zero_based()) % 2;
                    colors[usize::from(color_idx)].apply_to(
                        format_square(match grid[coord] {
                            Some(piece) => to_unicode_char(piece.kind, piece.force),
                            None => ' ',
                        })
                    ).to_string()
                }
            };
            ret.push_str(&square);
        }
        ret.push('\n');
    }
    ret
}

fn format_square(ch: char) -> String {
    format!(" {} ", ch)
}

fn to_unicode_char(piece_kind: PieceKind, force: Force) -> char {
    use self::PieceKind::*;
    use self::Force::*;
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
