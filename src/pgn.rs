use std::rc::Rc;
use std::time::Duration;

use enum_map::enum_map;
use itertools::Itertools;
use serde::{Serialize, Deserialize};
use strum::IntoEnumIterator;

use crate::board::Board;
use crate::clock::TimeControl;
use crate::fen;
use crate::force::Force;
use crate::game::{TurnRecord, BughousePlayerId, BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::grid::Grid;
use crate::player::{Team, Player};
use crate::util::div_ceil_u128;


#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum BughouseExportFormat {
    // BPGN (Bughouse Portable Game Notation) format designed specifically for bughouse.
    // Keep accurate track of relative turn order on the two board, but it's well supported.
    // Doc: https://bughousedb.com/Lieven_BPGN_Standard.txt
    Bpgn,

    // PGN (Portable Game Notation) format is the de-facto standard plain format for
    // recording chess games. Bughouse game is stored as two separate games.
    // Doc: http://www.saremba.de/chessgml/standards/pgn/pgn-complete.htm
    PgnPair,
}

const LINE_WIDTH: usize = 80;

struct TextDocument {
    text: String,
    last_line_len: usize,
}
impl TextDocument {
    fn new() -> Self { TextDocument{ text: String::new(), last_line_len: 0 } }
    fn push_word(&mut self, word: &str) {
        if self.last_line_len == 0 {
            // no separators: first record
        } else if self.last_line_len + word.len() + 1 <= LINE_WIDTH {
            self.text.push(' ');
            self.last_line_len += 1;
        } else {
            self.text.push('\n');
            self.last_line_len = 0;
        }
        self.text.push_str(&word);
        self.last_line_len += word.len();
    }
    fn render(&self) -> String {
        let trailing_newline = if self.last_line_len > 0 { "\n" } else { "" };
        format!("{}{}", self.text, trailing_newline)
    }
}

fn ceil_seconds(duration: Duration) -> u128 {
    div_ceil_u128(duration.as_nanos(), 1_000_000_000)
}

fn time_control_to_string(control: &TimeControl) -> String {
    control.starting_time.as_secs().to_string()
}

fn make_result_string(game: &BughouseGame) -> &'static str {
    use BughouseGameStatus::*;
    match game.status() {
        Active => "*",
        Draw(_) => "1/2-1/2",
        Victory(team, _) => match team {
            Team::Red => "1-0",
            Team::Blue => "0-1",
        },
    }
}

// Dummy player object. Will not appear anywhere in the produced PGN.
fn dummy_player(team: Team) -> Rc<Player> {
    Rc::new(Player{ name: format!("{:?}", team), team })
}

fn make_bughouse_board_png_header(starting_grid: &Grid, board: &Board, board_idx: BughouseBoard, round: usize)
    -> String
{
    use BughouseBoard::*;
    use Force::*;
    let now = chrono::offset::Utc::now();
    let starting_board = Board::new(
        board.chess_rules().clone(),
        board.bughouse_rules().clone(),
        enum_map!{ White => dummy_player(Team::Red), Black => dummy_player(Team::Blue) },
        starting_grid.clone()
    );
    let board_name = match board_idx {
        A => "A",
        B => "B",
    };
    format!(
r#"[Event "Friendly Bughouse Match"]
[Site "bughouse.pro"]
[UTCDate "{}"]
[UTCTime "{}"]
[Round "{}-{}"]
[White "{}"]
[Black "{}"]
[TimeControl "{}"]
[SetUp "1"]
[FEN "{}"]
[Result "*"]
"#,
        now.format("%Y.%m.%d"),
        now.format("%H:%M:%S"),
        round,
        board_name,
        board.player(White).name,
        board.player(Black).name,
        time_control_to_string(&board.chess_rules().time_control),
        fen::starting_position_to_shredder_fen(&starting_board),
    )
}

// Improvement potential. More human-readable "Termination" tag values.
fn make_bughouse_bpng_header(starting_grid: &Grid, game: &BughouseGame, round: usize) -> String {
    use BughouseBoard::*;
    use Force::*;
    let now = chrono::offset::Utc::now();
    let starting_board = Board::new(
        Rc::clone(game.chess_rules()),
        Some(Rc::clone(game.bughouse_rules())),
        enum_map!{ White => dummy_player(Team::Red), Black => dummy_player(Team::Blue) },
        starting_grid.clone()
    );
    let starting_position_fen = fen::starting_position_to_shredder_fen(&starting_board);
    format!(
r#"[Event "Friendly Bughouse Match"]
[Site "bughouse.pro"]
[UTCDate "{}"]
[UTCTime "{}"]
[Round "{}"]
[WhiteA "{}"]
[BlackA "{}"]
[WhiteB "{}"]
[BlackB "{}"]
[TimeControl "{}"]
[Variant "Bughouse"]
[SetUp "1"]
[FEN "{} | {}"]
[Result "{}"]
[Termination "{:?}"]
"#,
        now.format("%Y.%m.%d"),
        now.format("%H:%M:%S"),
        round,
        game.board(A).player(White).name,
        game.board(A).player(Black).name,
        game.board(B).player(White).name,
        game.board(B).player(Black).name,
        time_control_to_string(&game.chess_rules().time_control),
        starting_position_fen, starting_position_fen,
        make_result_string(game),
        game.status(),
    )
}

fn player_notation(player_id: BughousePlayerId) -> &'static str {
    use BughouseBoard::*;
    use Force::*;
    match (player_id.board_idx, player_id.force) {
        (A, White) => "A",
        (A, Black) => "a",
        (B, White) => "B",
        (B, Black) => "b",
    }
}

pub fn export_to_pgn_pair(starting_grid: &Grid, game: &BughouseGame, round: usize)
    -> String
{
    BughouseBoard::iter().map(|board_idx| {
        let board = game.board(board_idx);
        let header = make_bughouse_board_png_header(starting_grid, board, board_idx, round);
        let mut doc = TextDocument::new();
        let mut full_turn_idx = 1;
        let turn_record_pair_iter = game.turn_log().iter().filter(|record| {
            record.player_id.board_idx == board_idx
        }).chunks(2);
        for turn_record_pair in &turn_record_pair_iter {
            let turn_notation = format!(
                "{}. {}",
                full_turn_idx,
                turn_record_pair.map(|record| &record.turn_algebraic).join(" "),
            );
            full_turn_idx += 1;
            doc.push_word(&turn_notation);
        }
        format!("{}{}", header, doc.render())
    }).join("\n")
}

pub fn export_to_bpgn(starting_grid: &Grid, game: &BughouseGame, round: usize) -> String {
    let header = make_bughouse_bpng_header(starting_grid, game, round);
    let total_time = game.chess_rules().time_control.starting_time;
    let mut doc = TextDocument::new();
    let mut full_turn_idx = enum_map!{ _ => 1 };
    for turn_record in game.turn_log() {
        let TurnRecord{ player_id, turn_algebraic, time } = turn_record;
        let time_left = total_time - time.elapsed_since_start();
        let turn_notation = format!(
            "{}{}. {} {{{}}}",
            full_turn_idx[player_id.board_idx],
            player_notation(*player_id),
            turn_algebraic,
            // Improvement potential: Consider using chess.com time format: "{[%clk 0:00:07.9]}".
            ceil_seconds(time_left),
        );
        if player_id.force == Force::Black {
            full_turn_idx[player_id.board_idx] += 1;
        }
        doc.push_word(&turn_notation);
    }
    format!("{}{}", header, doc.render())
}

pub fn export_bughouse(format: BughouseExportFormat, starting_grid: &Grid, game: &BughouseGame, round: usize)
    -> String
{
    use BughouseExportFormat::*;
    match format {
        Bpgn => export_to_bpgn(starting_grid, game, round),
        PgnPair => export_to_pgn_pair(starting_grid, game, round),
    }
}
