use enum_map::enum_map;
use indoc::formatdoc;
use serde::{Serialize, Deserialize};
use time::macros::format_description;

use crate::algebraic::{AlgebraicDetails, AlgebraicCharset};
use crate::board::{TurnInput, TurnMode, VictoryReason, DrawReason};
use crate::clock::TimeControl;
use crate::{fen, ChessVariant, FairyPieces};
use crate::force::Force;
use crate::game::{TurnRecordExpanded, BughouseEnvoy, BughouseBoard, BughouseGameStatus, BughouseGame};
use crate::player::Team;
use crate::rules::StartingPosition;


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
        const SPACE_WIDTH: usize = 1;
        if self.last_line_len == 0 {
            // no separators: first record
        } else if self.last_line_len + word.len() + SPACE_WIDTH <= LINE_WIDTH {
            self.text.push(' ');
            self.last_line_len += SPACE_WIDTH;
        } else {
            self.text.push('\n');
            self.last_line_len = 0;
        }
        self.text.push_str(word);
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

fn make_termination_string(game: &BughouseGame) -> &'static str {
    use BughouseGameStatus::*;
    use VictoryReason::*;
    use DrawReason::*;
    match game.status() {
        Active => "unterminated",
        Victory(_, Checkmate) => "normal",
        Victory(_, Flag) => "time forfeit",
        // There is no "resign" Termination, should use "normal" apparently:
        // https://lichess.org/forum/general-chess-discussion/how-do-i-make-it-say-that-one-side-resigned#4
        Victory(_, Resignation) => "normal",
        // Somehow I'm skeptical many chess engines would be prepared for a "time forfeit" draw
        Draw(SimultaneousFlag) => "normal",
        Draw(ThreefoldRepetition) => "normal",
    }
}

fn make_bughouse_bpng_header(game: &BughouseGame, game_at_start: &BughouseGame, round: usize) -> String {
    use BughouseBoard::*;
    use Force::*;
    // TODO: Save game start time instead.
    let now = time::OffsetDateTime::now_utc();
    let mut variant = vec!["Bughouse"];
    let mut setup = String::new();
    match game.chess_rules().starting_position {
        StartingPosition::Classic => {},
        StartingPosition::FischerRandom => {
            variant.push("Chess960");
            let a = fen::starting_position_to_shredder_fen(&game_at_start.board(BughouseBoard::A));
            let b = fen::starting_position_to_shredder_fen(&game_at_start.board(BughouseBoard::B));
            setup = format!("[SetUp \"1\"]\n[FEN \"{a} | {b}\"]\n");
        }
    }
    match game.chess_rules().chess_variant {
        ChessVariant::Standard => {},
        ChessVariant::FogOfWar => { variant.push("DarkChess"); },
    }
    match game.chess_rules().fairy_pieces {
        FairyPieces::NoFairy => {},
        FairyPieces::Accolade => { variant.push("Accolade"); },
    }
    let event = if game.contest_rules().rated { "Rated Bughouse Match" } else { "Unrated Bughouse Match" };
    formatdoc!(r#"
        [Event "{}"]
        [Site "bughouse.pro"]
        [UTCDate "{}"]
        [UTCTime "{}"]
        [Round "{}"]
        [WhiteA "{}"]
        [BlackA "{}"]
        [WhiteB "{}"]
        [BlackB "{}"]
        [TimeControl "{}"]
        [Variant "{}"]
        [DropAggression "{}"]
        [PawnDropRanks "{}"]
        {}[Result "{}"]
        [Termination "{}"]
        [Outcome "{}"]
        "#,
        event,
        now.format(format_description!("[year].[month].[day]")).unwrap(),
        now.format(format_description!("[hour]:[minute]:[second]")).unwrap(),
        round,
        game.board(A).player_name(White),
        game.board(A).player_name(Black),
        game.board(B).player_name(White),
        game.board(B).player_name(Black),
        time_control_to_string(&game.chess_rules().time_control),
        variant.join(" "),
        game.bughouse_rules().drop_aggression_string(),
        game.bughouse_rules().pawn_drop_ranks_string(),
        setup,
        make_result_string(game),
        make_termination_string(game),
        game.outcome(),
    )
}

fn envoy_notation(envoy: BughouseEnvoy) -> &'static str {
    use BughouseBoard::*;
    use Force::*;
    match (envoy.board_idx, envoy.force) {
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
//
// Also contains non-standard extension fields:
//   - "Variant" - follow chess.com example;
//   - "Outcome" - human-readable game result description; this is addition to "Result"
//     and "Termination" fields, which follow PGN standard, but are less informative.
pub fn export_to_bpgn(_format: BughouseExportFormat, game: &BughouseGame, round: usize)
    -> String
{
    let mut game_rerun = game.clone_from_start();
    let header = make_bughouse_bpng_header(game, &game_rerun, round);
    let mut doc = TextDocument::new();
    let mut full_turn_idx = enum_map!{ _ => 1 };
    for turn_record in game.turn_log() {
        // TODO: Just use the turn log now that representation can be chosen on the fly.
        let TurnRecordExpanded{ envoy, turn_expanded, time, .. } = turn_record;
        let turn_algebraic = game_rerun.board(envoy.board_idx).turn_to_algebraic(
            turn_expanded.turn,
            TurnMode::Normal,
            AlgebraicDetails::ShortAlgebraic,
        ).unwrap();
        game_rerun.try_turn_by_envoy(
            *envoy,
            &TurnInput::Explicit(turn_expanded.turn),
            TurnMode::Normal,
            *time,
        ).unwrap();
        let turn_notation = format!(
            "{}{}. {}",
            full_turn_idx[envoy.board_idx],
            envoy_notation(*envoy),
            turn_algebraic.format(AlgebraicCharset::Ascii),
        );
        if envoy.force == Force::Black {
            full_turn_idx[envoy.board_idx] += 1;
        }
        doc.push_word(&turn_notation);
    }
    format!("{}{}", header, doc.render())
}
