use std::iter;

use indoc::formatdoc;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use time::macros::format_description;

use crate::algebraic::AlgebraicCharset;
use crate::board::{DrawReason, VictoryReason};
use crate::clock::TimeControl;
use crate::fen;
use crate::force::Force;
use crate::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus, TurnRecordExpanded,
};
use crate::player::Team;
use crate::rules::{ChessVariant, StartingPosition};
use crate::utc_time::UtcDateTime;

// Other possible formats:
//
//   - Storing timestamp with the original precision is an obvious choice, but nanoseconds are
//     noisy and I don't think we ever really need this. The best reason for original precision
//     is that it guarantees perfect correspondence between in-game and post-game replayes. In
//     practice it seems feasible to completely avoid rounding mismatches: `time_breakdown` test
//     in `clock.rs` verifies that.
//
//   - https://bughousedb.com/Lieven_BPGN_Standard.txt shows remaining clock time as whole
//     seconds in braces like this:
//       1A. d4 {298} 1a. e6 {298} 2A. e4 {296} 2a. Nf6 {297}
//     I don't really like it because seconds precision is too low and syntax is ambiguous.
//
//   - chess.com shows remaining clock in pretty printed format like this:
//       {[%clk 0:00:07.9]}
//
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum BpgnTimeFormat {
    NoTime,

    // Seconds since the start of the game with milliseconds precision. Example:
    //   {[ts=185.070]}
    Timestamp,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BpgnExportFormat {
    pub time_format: BpgnTimeFormat,
}

impl Default for BpgnExportFormat {
    fn default() -> Self { BpgnExportFormat { time_format: BpgnTimeFormat::Timestamp } }
}

const LINE_WIDTH: usize = 80;

struct TextDocument {
    text: String,
    last_line_len: usize,
}
impl TextDocument {
    fn new() -> Self { TextDocument { text: String::new(), last_line_len: 0 } }
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

fn make_event(game: &BughouseGame) -> &'static str {
    if game.match_rules().rated {
        "Rated Bughouse Match"
    } else {
        "Unrated Bughouse Match"
    }
}

// Improvement potential: Convert `EffectiveStartingPosition`to FEN directly.
fn make_setup_tag(game: &BughouseGame) -> String {
    let game_at_start = game.clone_from_start();
    match game.chess_rules().starting_position {
        StartingPosition::Classic => String::new(),
        StartingPosition::FischerRandom => {
            let a = fen::starting_position_to_shredder_fen(game_at_start.board(BughouseBoard::A));
            let b = fen::starting_position_to_shredder_fen(game_at_start.board(BughouseBoard::B));
            format!("[SetUp \"1\"]\n[FEN \"{a} | {b}\"]\n")
        }
    }
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
    use DrawReason::*;
    use VictoryReason::*;
    match game.status() {
        Active => "unterminated",
        Victory(_, Checkmate) => "normal",
        Victory(_, Flag) => "time forfeit",
        // There is no "resign" Termination, should use "normal" apparently:
        // https://lichess.org/forum/general-chess-discussion/how-do-i-make-it-say-that-one-side-resigned#4
        Victory(_, Resignation) => "normal",
        Draw(SimultaneousCheckmate) => "normal",
        // Somehow I'm skeptical many chess engines would be prepared for a "time forfeit" draw
        Draw(SimultaneousFlag) => "normal",
        Draw(ThreefoldRepetition) => "normal",
    }
}

// TODO(duck): Improve duck notation. Here's the suggested notation:
//   https://duckchess.com/#:~:text=Finally%2C%20the%20standard%20notation%20for,duck%20being%20placed%20at%20g5.
// Note that it interacts questionably with bughouse, because it reuses the '@' symbol.
// On the other hand, it's still unambiguous, so maybe it's ok.
fn make_bughouse_bpng_header(
    game: &BughouseGame, game_start_time: UtcDateTime, round: u64,
) -> String {
    use BughouseBoard::*;
    use Force::*;
    let now = time::OffsetDateTime::from(game_start_time);
    let event = make_event(game);
    let variants = iter::once("Bughouse")
        .chain(game.chess_rules().variants().into_iter().map(ChessVariant::to_pgn))
        .collect_vec();
    let setup_tag = make_setup_tag(game);
    formatdoc!(
        r#"
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
        [Promotion "{}"]
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
        variants.join(" "),
        game.bughouse_rules().promotion_string(),
        game.bughouse_rules().drop_aggression_string(),
        game.bughouse_rules().pawn_drop_ranks_string(),
        setup_tag,
        make_result_string(game),
        make_termination_string(game),
        game.outcome().to_readable_string(game.chess_rules()),
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

fn turn_to_pgn(format: BpgnExportFormat, game: &BughouseGame, turn: &TurnRecordExpanded) -> String {
    let TurnRecordExpanded {
        local_number, envoy, turn_expanded, time, ..
    } = turn;
    let mut s = format!(
        "{}{}. {}",
        local_number,
        envoy_notation(*envoy),
        turn_expanded.algebraic.format(game.board_shape(), AlgebraicCharset::Ascii),
    );
    match format.time_format {
        BpgnTimeFormat::NoTime => {}
        BpgnTimeFormat::Timestamp => {
            s.push_str(&format!(" {{[ts={:.3}]}}", time.to_pgn_timestamp()));
        }
    }
    s
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
//   - "Promotion", "DropAggression", "PawnDropRanks" - bughouse-specific rules.
pub fn export_to_bpgn(
    format: BpgnExportFormat, game: &BughouseGame, game_start_time: UtcDateTime, round: u64,
) -> String {
    let header = make_bughouse_bpng_header(game, game_start_time, round);
    let mut doc = TextDocument::new();
    for turn_record in game.turn_log() {
        doc.push_word(&turn_to_pgn(format, game, &turn_record));
    }
    format!("{}{}", header, doc.render())
}


#[cfg(test)]
mod tests {
    use std::time::Duration;

    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use time::macros::datetime;

    use super::*;
    use crate::role::Role;
    use crate::rules::{ChessRules, MatchRules, Rules};
    use crate::test_util::{replay_bughouse_log, sample_bughouse_players};

    #[test]
    fn pgn_golden() {
        let rules = Rules {
            match_rules: MatchRules::unrated(),
            chess_rules: ChessRules::bughouse_chess_com(),
        };
        let mut game =
            BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
        replay_bughouse_log(
            &mut game,
            "
                1A.e4 1a.Nc6 1B.d4 2A.Nc3 1b.Nf6 2a.Nf6 2B.d5 3A.d4 2b.e6 3a.d5 3B.dxe6 4A.e5
                3b.dxe6 4B.Qxd8 4a.Ne4 4b.Kxd8 5B.Bg5 5A.Nxe4 5a.dxe4 5b.Be7 6A.Nh3 6B.Nc3
                6a.Bxh3 6b.N@d4 7A.gxh3 7a.Nxd4 7B.O-O-O 8A.P@e6 7b.Nbc6 8B.Bxf6 8a.N@f3 9A.Qxf3
                8b.Bxf6 9a.Nxf3 10A.Ke2 9B.e3 10a.Q@d2 11A.Bxd2 11a.Qxd2
            ",
            Duration::from_millis(100),
        )
        .unwrap();
        let game_start_time = UtcDateTime::from(datetime!(2024-03-06 13:37));
        let bpgn = export_to_bpgn(BpgnExportFormat::default(), &game, game_start_time, 1);
        assert_eq!(
            bpgn,
            indoc!(
                r#"
                [Event "Unrated Bughouse Match"]
                [Site "bughouse.pro"]
                [UTCDate "2024.03.06"]
                [UTCTime "13:37:00"]
                [Round "1"]
                [WhiteA "Alice"]
                [BlackA "Bob"]
                [WhiteB "Charlie"]
                [BlackB "Dave"]
                [TimeControl "300"]
                [Variant "Bughouse"]
                [Promotion "Upgrade"]
                [DropAggression "Mate allowed"]
                [PawnDropRanks "2-7"]
                [Result "0-1"]
                [Termination "normal"]
                [Outcome "Bob & Charlie won: Alice & Dave checkmated"]
                1A. e4 {[ts=0.0]} 1a. Nc6 {[ts=0.1]} 1B. d4 {[ts=0.2]} 2A. Nc3 {[ts=0.3]}
                1b. Nf6 {[ts=0.4]} 2a. Nf6 {[ts=0.5]} 2B. d5 {[ts=0.6]} 3A. d4 {[ts=0.7]}
                2b. e6 {[ts=0.8]} 3a. d5 {[ts=0.9]} 3B. xe6 {[ts=1.0]} 4A. e5 {[ts=1.1]}
                3b. dxe6 {[ts=1.2]} 4B. Qxd8 {[ts=1.3]} 4a. Ne4 {[ts=1.4]} 4b. Kxd8 {[ts=1.5]}
                5B. Bg5 {[ts=1.6]} 5A. Nxe4 {[ts=1.7]} 5a. xe4 {[ts=1.8]} 5b. Be7 {[ts=1.9]}
                6A. Nh3 {[ts=2.0]} 6B. Nc3 {[ts=2.1]} 6a. Bxh3 {[ts=2.2]} 6b. N@d4 {[ts=2.3]}
                7A. xh3 {[ts=2.4]} 7a. Nxd4 {[ts=2.5]} 7B. O-O-O {[ts=2.6]} 8A. P@e6 {[ts=2.7]}
                7b. N8c6 {[ts=2.8]} 8B. Bxf6 {[ts=2.9]} 8a. N@f3 {[ts=3.0]} 9A. Qxf3 {[ts=3.1]}
                8b. Bxf6 {[ts=3.2]} 9a. Nxf3 {[ts=3.3]} 10A. Ke2 {[ts=3.4]} 9B. e3 {[ts=3.5]}
                10a. Q@d2 {[ts=3.6]} 11A. Bxd2 {[ts=3.7]} 11a. Qxd2 {[ts=3.8]}
                "#
            )
        );
    }
}
