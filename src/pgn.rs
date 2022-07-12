use std::rc::Rc;

use enum_map::enum_map;
use serde::{Serialize, Deserialize};

use crate::board::Board;
use crate::clock::TimeControl;
use crate::fen;
use crate::force::Force;
use crate::game::{TurnRecord, BughousePlayerId, BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::grid::Grid;
use crate::player::{Team, Player};


#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BughouseExportFormat {
    // TODO: Support time export and allow to choose format here:
    //   - as described in https://bughousedb.com/Lieven_BPGN_Standard.txt, but export
    //       with milliseconds precision, like https://bughousedb.com itself does.
    //   - as in chess.com, e.g. "{[%clk 0:00:07.9]}"
    //   - precise
    //   - none
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

// Exports to BPGN (Bughouse Portable Game Notation) - format designed specifically for
// bughouse. Doc: https://bughousedb.com/Lieven_BPGN_Standard.txt
// Based on PGN (Portable Game Notation), the de-facto standard plain format for recording
// chess games. Doc: http://www.saremba.de/chessgml/standards/pgn/pgn-complete.htm
pub fn export_to_bpgn(_format: BughouseExportFormat, starting_grid: &Grid, game: &BughouseGame, round: usize)
    -> String
{
    let header = make_bughouse_bpng_header(starting_grid, game, round);
    let mut doc = TextDocument::new();
    let mut full_turn_idx = enum_map!{ _ => 1 };
    for turn_record in game.turn_log() {
        let TurnRecord{ player_id, turn_algebraic, .. } = turn_record;
        let turn_notation = format!(
            "{}{}. {}",
            full_turn_idx[player_id.board_idx],
            player_notation(*player_id),
            turn_algebraic,
        );
        if player_id.force == Force::Black {
            full_turn_idx[player_id.board_idx] += 1;
        }
        doc.push_word(&turn_notation);
    }
    format!("{}{}", header, doc.render())
}
