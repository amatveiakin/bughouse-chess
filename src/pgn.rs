use std::collections::{HashMap, HashSet};
use std::iter;
use std::time::Duration;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use time::macros::format_description;

use crate::algebraic::AlgebraicCharset;
use crate::board::{DrawReason, TurnInput, TurnMode, VictoryReason};
use crate::clock::{GameInstant, TimeControl};
use crate::fen;
use crate::force::Force;
use crate::game::{
    get_bughouse_board, BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus,
    BughousePlayer, GameOutcome, PlayerInGame,
};
use crate::player::Team;
use crate::role::Role;
use crate::rules::{
    BughouseRules, ChessRules, ChessVariant, DropAggression, FairyPieces, MatchRules,
    PawnDropRanks, Promotion, Rules, StartingPosition,
};
use crate::starter::EffectiveStartingPosition;
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


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReducedBughouseGameStatus {
    Active,
    Victory(Team),
    Draw,
}
impl From<BughouseGameStatus> for ReducedBughouseGameStatus {
    fn from(status: BughouseGameStatus) -> Self {
        use BughouseGameStatus::*;
        match status {
            Active => ReducedBughouseGameStatus::Active,
            Victory(team, _) => ReducedBughouseGameStatus::Victory(team),
            Draw(_) => ReducedBughouseGameStatus::Draw,
        }
    }
}

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

#[derive(Clone, PartialEq, Eq, Debug)]
enum Token<'a> {
    // Square bracket, parenthesis or angle bracket.
    Bracket(char),
    // Any characters enclosed in double quotes.
    QuotedString(&'a str),
    // Sequence of characters that are not:
    //   - whitespace
    //   - any kind of bracket,
    //   - double quotes.
    Word(&'a str),
    // Special comment with {[...]} syntax used to add additional information to a turn.
    Addendum(&'a str),
    // Regular comments are ignored.
}

type TokenIter<'a> = iter::Peekable<std::vec::IntoIter<Token<'a>>>;

fn tokenize_bpgn(text: &str) -> Vec<Token<'_>> {
    #[derive(Clone, Copy)]
    enum UnfinishedToken {
        QuotedString,
        Word,
        BraceComment,
        SemicolonComment,
        PercentComment,
    }
    #[derive(Clone, Copy)]
    enum CharAction {
        Consume,
        Keep,
    }
    use CharAction::*;

    let mut tokens = vec![];
    let mut unfinished_token = None;
    let mut token_start = None;
    let mut new_line = true;
    for (i, ch) in text.char_indices() {
        if let Some(unfinished_token_value) = unfinished_token {
            if token_start.is_none() {
                // A trick to allow starting token contest after the opening character.
                token_start = Some(i);
            }
            let start = token_start.unwrap();
            let mut result = None;
            match unfinished_token_value {
                UnfinishedToken::QuotedString => {
                    if ch == '"' {
                        result = Some((Some(Token::QuotedString(&text[start..i])), Consume));
                    }
                }
                UnfinishedToken::Word => {
                    if ch.is_whitespace() || "{}[]()<>\"".contains(ch) {
                        result = Some((Some(Token::Word(&text[start..i])), Keep));
                    }
                }
                UnfinishedToken::BraceComment => {
                    if ch == '}' {
                        let token = text[start..i]
                            .strip_prefix('[')
                            .and_then(|s| s.strip_suffix(']'))
                            .map(|s| Token::Addendum(s));
                        result = Some((token, Consume));
                    }
                }
                UnfinishedToken::SemicolonComment => {
                    if ch == '\n' {
                        result = Some((None, Keep));
                    }
                }
                UnfinishedToken::PercentComment => {
                    if ch == '\n' {
                        result = Some((None, Keep));
                    }
                }
            }
            if let Some((token, char_action)) = result {
                if let Some(token) = token {
                    tokens.push(token);
                }
                unfinished_token = None;
                token_start = None;
                match char_action {
                    Consume => continue,
                    Keep => {}
                }
            }
        }
        if unfinished_token.is_none() {
            assert!(token_start.is_none());
            match ch {
                ch if ch.is_whitespace() => {}
                '[' | ']' | '(' | ')' | '<' | '>' => {
                    tokens.push(Token::Bracket(ch));
                }
                '{' => {
                    unfinished_token = Some(UnfinishedToken::BraceComment);
                }
                '"' => {
                    unfinished_token = Some(UnfinishedToken::QuotedString);
                }
                ';' => {
                    unfinished_token = Some(UnfinishedToken::SemicolonComment);
                }
                '%' if new_line => {
                    unfinished_token = Some(UnfinishedToken::PercentComment);
                }
                _ => {
                    unfinished_token = Some(UnfinishedToken::Word);
                    token_start = Some(i);
                }
            }
        }
        new_line = ch == '\n';
    }
    tokens
}

struct BpgnHeader {
    tags: Vec<(String, String)>,
}

struct BpgnTurn {
    number: u32,
    envoy: BughouseEnvoy,
    algebraic: String,
    addenda: Vec<(String, String)>,
}

struct BpgnBody {
    turns: Vec<BpgnTurn>,
}

struct BpgnDocument {
    header: BpgnHeader,
    body: BpgnBody,
}

impl BpgnHeader {
    fn new() -> Self { BpgnHeader { tags: Vec::new() } }
    fn push_tag(&mut self, key: impl ToString, value: impl ToString) {
        self.tags.push((key.to_string(), value.to_string()));
    }
    fn render(&self) -> String {
        self.tags
            .iter()
            .map(|(key, value)| format!("[{} \"{}\"]\n", key, value))
            .collect()
    }
    fn parse(tokens: &mut TokenIter<'_>) -> Result<Self, &'static str> {
        let mut result = BpgnHeader::new();
        while tokens.next_if_eq(&Token::Bracket('[')).is_some() {
            let Some(Token::Word(key)) = tokens.next() else {
                return Err("missing tag key");
            };
            let Some(Token::QuotedString(value)) = tokens.next() else {
                return Err("missing tag value");
            };
            if tokens.next_if_eq(&Token::Bracket(']')).is_none() {
                return Err("missing closing bracket");
            }
            result.tags.push((key.to_string(), value.to_string()));
        }
        Ok(result)
    }
}

impl BpgnTurn {
    fn render_without_addenda(&self) -> String {
        format!("{}{}. {}", self.number, envoy_notation(self.envoy), self.algebraic)
    }
    fn render(&self) -> String {
        let mut result = self.render_without_addenda();
        for add in &self.addenda {
            result.push_str(&format!(" {{[{}={}]}}", add.0, add.1));
        }
        result
    }
    fn parse(tokens: &mut TokenIter<'_>) -> Result<Self, &'static str> {
        let Some(Token::Word(pre)) = tokens.next() else {
            return Err("missing turn number");
        };
        let pre = pre.strip_suffix('.').ok_or("turn number must end with a dot")?;
        let (number, envoy) = pre.split_at(pre.len() - 1); // ok: envoy notation is always ASCII
        let number = number.parse().map_err(|_| "invalid turn number")?;
        let envoy = parse_envoy_notation(envoy).ok_or("invalid player notation")?;
        let Some(Token::Word(algebraic)) = tokens.next() else {
            return Err("missing algebraic notation");
        };
        let algebraic = algebraic.to_owned();
        let mut addenda = vec![];
        while let Some(t) = tokens.next_if(|t| matches!(t, Token::Addendum(_))) {
            let Token::Addendum(addendum) = t else {
                unreachable!()
            };
            let (key, value) =
                addendum.split_once('=').ok_or("turn addendum must have {[key=value]} format")?;
            addenda.push((key.to_string(), value.to_string()));
        }
        Ok(BpgnTurn { number, envoy, algebraic, addenda })
    }
}

impl BpgnBody {
    fn new() -> Self { BpgnBody { turns: Vec::new() } }
    fn push_turn(&mut self, turn: BpgnTurn) { self.turns.push(turn); }
    fn render(&self) -> String {
        let mut doc = TextDocument::new();
        for turn in &self.turns {
            doc.push_word(&turn.render());
        }
        doc.render()
    }
    fn parse(tokens: &mut TokenIter<'_>) -> Result<Self, &'static str> {
        let mut result = BpgnBody::new();
        while tokens.peek().is_some() {
            result.push_turn(BpgnTurn::parse(tokens)?);
        }
        Ok(result)
    }
}

impl BpgnDocument {
    fn render(&self) -> String { format!("{}{}", self.header.render(), self.body.render()) }
    fn parse(s: &str) -> Result<Self, &'static str> {
        let tokens = tokenize_bpgn(s);
        let mut iter: iter::Peekable<std::vec::IntoIter<Token<'_>>> = tokens.into_iter().peekable();
        let header = BpgnHeader::parse(&mut iter)?;
        let body = BpgnBody::parse(&mut iter)?;
        Ok(BpgnDocument { header, body })
    }
}

struct TagMap {
    map: HashMap<String, String>,
}

impl TagMap {
    fn get(&self, key: &str) -> Result<&str, String> {
        self.map
            .get(key)
            .map(|s| s.as_str())
            .ok_or_else(|| format!("missing {key} tag"))
    }
    fn get_and_parse<T>(
        &self, key: &str, parse: impl FnOnce(&str) -> Option<T>,
    ) -> Result<T, String> {
        parse(self.get(key)?).ok_or_else(|| format!("cannot parse {key} tag"))
    }
}

fn render_time_control(control: &TimeControl) -> String {
    control.starting_time.as_secs().to_string()
}
fn parse_time_control(s: &str) -> Result<TimeControl, &'static str> {
    let seconds = s.parse().map_err(|_| "invalid time control")?;
    Ok(TimeControl {
        starting_time: Duration::from_secs(seconds),
    })
}

fn make_event(game: &BughouseGame) -> &'static str {
    if game.match_rules().rated {
        "Rated Bughouse Match"
    } else {
        "Unrated Bughouse Match"
    }
}

fn make_result_string(status: BughouseGameStatus) -> &'static str {
    use BughouseGameStatus::*;
    match status {
        Active => "*",
        Draw(_) => "1/2-1/2",
        Victory(Team::Red, _) => "1-0",
        Victory(Team::Blue, _) => "0-1",
    }
}
fn parse_game_result(result: &str) -> Result<ReducedBughouseGameStatus, String> {
    match result {
        "*" => Ok(ReducedBughouseGameStatus::Active),
        "1-0" => Ok(ReducedBughouseGameStatus::Victory(Team::Red)),
        "0-1" => Ok(ReducedBughouseGameStatus::Victory(Team::Blue)),
        "1/2-1/2" => Ok(ReducedBughouseGameStatus::Draw),
        _ => Err(format!("invalid game result: {result}")),
    }
}

fn make_termination_string(status: BughouseGameStatus) -> &'static str {
    use BughouseGameStatus::*;
    use DrawReason::*;
    use VictoryReason::*;
    match status {
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

fn parse_envoy_notation(s: &str) -> Option<BughouseEnvoy> {
    use BughouseBoard::*;
    use Force::*;
    match s {
        "A" => Some(BughouseEnvoy { board_idx: A, force: White }),
        "a" => Some(BughouseEnvoy { board_idx: A, force: Black }),
        "B" => Some(BughouseEnvoy { board_idx: B, force: White }),
        "b" => Some(BughouseEnvoy { board_idx: B, force: Black }),
        _ => None,
    }
}

fn total_game_duration(game: &BughouseGame) -> Option<GameInstant> {
    // Note. Cannot use `turn_log()` because it does not record time forfeits and resignations.
    // TODO: Store latest game event time and use it here and in `current_game_time` in `server.rs`.
    if game.status().is_active() {
        return None;
    }
    Some(GameInstant::from_game_duration(
        BughouseBoard::iter()
            .map(|board_idx| game.board(board_idx).clock().total_time_elapsed())
            .all_equal_value()
            .unwrap(),
    ))
}

// TODO(duck): Improve duck notation. Here's the suggested notation:
//   https://duckchess.com/#:~:text=Finally%2C%20the%20standard%20notation%20for,duck%20being%20placed%20at%20g5.
// Note that it interacts questionably with bughouse, because it reuses the '@' symbol.
// On the other hand, it's still unambiguous, so maybe it's ok.
fn make_bughouse_bpng_header(
    game: &BughouseGame, game_start_time: UtcDateTime, round: u64,
) -> BpgnHeader {
    use BughouseBoard::*;
    use Force::*;
    let now = time::OffsetDateTime::from(game_start_time);
    let game_at_start = game.clone_from_start();
    let variants = iter::once("Bughouse")
        .chain(game.chess_rules().variants().into_iter().map(ChessVariant::to_pgn))
        .collect_vec();

    let mut h = BpgnHeader::new();
    h.push_tag("Event", make_event(game));
    h.push_tag("Site", "bughouse.pro");
    h.push_tag("UTCDate", now.format(format_description!("[year].[month].[day]")).unwrap());
    h.push_tag("UTCTime", now.format(format_description!("[hour]:[minute]:[second]")).unwrap());
    h.push_tag("Round", round);
    h.push_tag("WhiteA", game.board(A).player_name(White));
    h.push_tag("BlackA", game.board(A).player_name(Black));
    h.push_tag("WhiteB", game.board(B).player_name(White));
    h.push_tag("BlackB", game.board(B).player_name(Black));
    h.push_tag("TimeControl", render_time_control(&game.chess_rules().time_control));
    h.push_tag("Variant", variants.join(" "));
    h.push_tag("Promotion", game.bughouse_rules().promotion.to_pgn());
    h.push_tag("DropAggression", game.bughouse_rules().drop_aggression.to_pgn());
    h.push_tag("PawnDropRanks", game.bughouse_rules().pawn_drop_ranks.to_pgn());
    match game.chess_rules().starting_position {
        StartingPosition::Classic => {}
        StartingPosition::FischerRandom => {
            // Improvement potential: Convert `EffectiveStartingPosition`to FEN directly.
            let a = fen::starting_position_to_shredder_fen(game_at_start.board(BughouseBoard::A));
            let b = fen::starting_position_to_shredder_fen(game_at_start.board(BughouseBoard::B));
            h.push_tag("SetUp", "1");
            h.push_tag("FEN", format!("{a} | {b}"));
        }
    }
    h.push_tag("Result", make_result_string(game.status()));
    h.push_tag("Termination", make_termination_string(game.status()));
    h.push_tag("Outcome", game.outcome().to_readable_string(game.chess_rules()));
    if let Some(d) = total_game_duration(game) {
        if let Some(ts) = d.to_pgn_timestamp() {
            h.push_tag("GameDuration", ts);
        }
    }
    h
}

// Exports to BPGN (Bughouse Portable Game Notation) - format designed specifically for
// bughouse. Doc: https://bughousedb.com/Lieven_BPGN_Standard.txt
// Based on PGN (Portable Game Notation), the de-facto standard format for recording
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
    let turns = game
        .turn_log()
        .iter()
        .map(|r| {
            let mut addenda = vec![];
            match format.time_format {
                BpgnTimeFormat::NoTime => {}
                BpgnTimeFormat::Timestamp => {
                    if let Some(ts) = r.time.to_pgn_timestamp() {
                        addenda.push(("ts".to_owned(), ts));
                    }
                }
            }
            BpgnTurn {
                number: r.local_number,
                envoy: r.envoy,
                algebraic: r
                    .turn_expanded
                    .algebraic
                    .format(game.board_shape(), AlgebraicCharset::Ascii),
                addenda,
            }
        })
        .collect();
    let body = BpgnBody { turns };
    let doc = BpgnDocument { header, body };
    doc.render()
}

fn parse_variants(s: &str) -> Result<HashSet<ChessVariant>, String> {
    let mut bughouse_found = false;
    let mut variants = HashSet::new();
    for word in s.split_whitespace() {
        if word == "Bughouse" {
            bughouse_found = true;
        } else {
            let v =
                ChessVariant::from_pgn(word).ok_or_else(|| format!("unknown variant: {}", word))?;
            variants.insert(v);
        }
    }
    if !bughouse_found {
        return Err("missing Bughouse variant".to_owned());
    }
    Ok(variants)
}

fn parse_rules(tags: &TagMap) -> Result<Rules, String> {
    let rated = match tags.get("Event")? {
        "Rated Bughouse Match" => true,
        "Unrated Bughouse Match" => false,
        _ => return Err("unexpected Event tag".to_owned()),
    };
    let time_control = parse_time_control(tags.get("TimeControl")?)?;
    let variants = parse_variants(tags.get("Variant")?)?;
    let starting_position = if variants.contains(&ChessVariant::FischerRandom) {
        StartingPosition::FischerRandom
    } else {
        StartingPosition::Classic
    };
    let fairy_pieces = if variants.contains(&ChessVariant::Accolade) {
        FairyPieces::Accolade
    } else {
        FairyPieces::NoFairy
    };
    let promotion = tags.get_and_parse("Promotion", Promotion::from_pgn)?;
    let pawn_drop_ranks = tags.get_and_parse("PawnDropRanks", PawnDropRanks::from_pgn)?;
    let drop_aggression = tags.get_and_parse("DropAggression", DropAggression::from_pgn)?;
    Ok(Rules {
        match_rules: MatchRules { rated },
        chess_rules: ChessRules {
            fairy_pieces,
            starting_position,
            duck_chess: variants.contains(&ChessVariant::DuckChess),
            atomic_chess: variants.contains(&ChessVariant::AtomicChess),
            fog_of_war: variants.contains(&ChessVariant::FogOfWar),
            time_control,
            bughouse_rules: Some(BughouseRules {
                koedem: variants.contains(&ChessVariant::Koedem),
                promotion,
                pawn_drop_ranks,
                drop_aggression,
            }),
        },
    })
}

fn parse_starting_position(
    rules: &ChessRules, tags: &TagMap,
) -> Result<EffectiveStartingPosition, String> {
    let setup_enabled = tags.get("SetUp") == Ok("1");
    let fen = tags.get("FEN");
    if setup_enabled != fen.is_ok() {
        return Err("SetUp and FEN tags must be used together".to_owned());
    }
    match rules.starting_position {
        StartingPosition::Classic => {}
        StartingPosition::FischerRandom => {
            if !setup_enabled {
                return Err("SetUp and FEN tags are mandatory for Fischer random".to_owned());
            }
        }
    }
    if !setup_enabled {
        return Ok(EffectiveStartingPosition::Classic);
    }
    let (a, b) = fen?.split_once('|').ok_or("invalid FEN")?;
    let mut boards = HashMap::new();
    boards.insert(BughouseBoard::A, fen::shredder_fen_to_starting_position(rules, a)?);
    boards.insert(BughouseBoard::B, fen::shredder_fen_to_starting_position(rules, b)?);
    // Q. Are there downsides to always using `ManualSetup` rather then `FischerRandom` for Fischer
    // random?
    Ok(EffectiveStartingPosition::ManualSetup(boards))
}

fn parse_players(tags: &TagMap) -> Result<Vec<PlayerInGame>, String> {
    use BughouseBoard::*;
    use Force::*;
    let mut player_names = HashMap::new();
    player_names.insert((White, A), tags.get("WhiteA")?);
    player_names.insert((Black, A), tags.get("BlackA")?);
    player_names.insert((White, B), tags.get("WhiteB")?);
    player_names.insert((Black, B), tags.get("BlackB")?);
    let mut players = vec![];
    for team in Team::iter() {
        let white_board = get_bughouse_board(team, White);
        let black_board = get_bughouse_board(team, Black);
        let white_name = player_names[&(White, white_board)].to_owned();
        let black_name = player_names[&(Black, black_board)].to_owned();
        if white_name == black_name {
            players.push(PlayerInGame {
                name: white_name,
                id: BughousePlayer::DoublePlayer(team),
            });
        } else {
            players.push(PlayerInGame {
                name: white_name,
                id: BughousePlayer::SinglePlayer(BughouseEnvoy {
                    force: White,
                    board_idx: white_board,
                }),
            });
            players.push(PlayerInGame {
                name: black_name,
                id: BughousePlayer::SinglePlayer(BughouseEnvoy {
                    force: Black,
                    board_idx: black_board,
                }),
            });
        }
    }
    Ok(players)
}

fn parse_game_status(
    players: &[PlayerInGame], tags: &TagMap,
) -> Result<BughouseGameStatus, String> {
    let outcome = GameOutcome::from_pgn(players, tags.get("Outcome")?)?;
    let status = outcome.status;
    let result = parse_game_result(tags.get("Result")?)?;
    if result != status.into() {
        return Err("inconsistent Outcome and Result tags".to_owned());
    }
    Ok(status)
}

fn parse_game_duration(tags: &TagMap) -> Result<GameInstant, String> {
    GameInstant::from_pgn_timestamp(tags.get("GameDuration")?)
        .map_err(|_| "invalid game duration".to_owned())
}

fn apply_turn(game: &mut BughouseGame, turn: BpgnTurn) -> Result<(), String> {
    let turn_time = match turn.addenda.iter().find(|(k, _)| k == "ts") {
        Some((_, ts)) => {
            GameInstant::from_pgn_timestamp(ts).map_err(|_| "invalid turn timestamp".to_owned())?
        }
        None => GameInstant::UNKNOWN,
    };
    game.try_turn_by_envoy(
        turn.envoy,
        &TurnInput::Algebraic(turn.algebraic.clone()),
        TurnMode::Normal,
        turn_time,
    )
    .map_err(|err| format!("turn {} invalid: {:?}", turn.render_without_addenda(), err))?;
    let turn_number = game.last_turn_record().unwrap().local_number;
    if turn_number != turn.number {
        return Err(format!("turn number mismatch: expected {}, got {}", turn_number, turn.number));
    }
    Ok(())
}

pub fn import_from_bpgn(s: &str, role: Role) -> Result<BughouseGame, String> {
    let doc = BpgnDocument::parse(s)?;
    let tags = TagMap {
        map: doc.header.tags.into_iter().collect(),
    };
    let rules = parse_rules(&tags)?;
    let starting_position = parse_starting_position(&rules.chess_rules, &tags)?;
    let players = parse_players(&tags)?;
    let status = parse_game_status(&players, &tags)?;
    let mut game =
        BughouseGame::new_with_starting_position(rules, role, starting_position, &players);
    for turn in doc.body.turns {
        apply_turn(&mut game, turn)?;
    }
    if !status.is_active() {
        let game_duration = parse_game_duration(&tags).unwrap_or_else(|_| GameInstant::UNKNOWN);
        game.set_status(status, game_duration);
    }
    Ok(game)
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
                [GameDuration "3.800"]
                1A. e4 {[ts=0.000]} 1a. Nc6 {[ts=0.100]} 1B. d4 {[ts=0.200]}
                2A. Nc3 {[ts=0.300]} 1b. Nf6 {[ts=0.400]} 2a. Nf6 {[ts=0.500]}
                2B. d5 {[ts=0.600]} 3A. d4 {[ts=0.700]} 2b. e6 {[ts=0.800]} 3a. d5 {[ts=0.900]}
                3B. xe6 {[ts=1.000]} 4A. e5 {[ts=1.100]} 3b. dxe6 {[ts=1.200]}
                4B. Qxd8 {[ts=1.300]} 4a. Ne4 {[ts=1.400]} 4b. Kxd8 {[ts=1.500]}
                5B. Bg5 {[ts=1.600]} 5A. Nxe4 {[ts=1.700]} 5a. xe4 {[ts=1.800]}
                5b. Be7 {[ts=1.900]} 6A. Nh3 {[ts=2.000]} 6B. Nc3 {[ts=2.100]}
                6a. Bxh3 {[ts=2.200]} 6b. N@d4 {[ts=2.300]} 7A. xh3 {[ts=2.400]}
                7a. Nxd4 {[ts=2.500]} 7B. O-O-O {[ts=2.600]} 8A. P@e6 {[ts=2.700]}
                7b. N8c6 {[ts=2.800]} 8B. Bxf6 {[ts=2.900]} 8a. N@f3 {[ts=3.000]}
                9A. Qxf3 {[ts=3.100]} 8b. Bxf6 {[ts=3.200]} 9a. Nxf3 {[ts=3.300]}
                10A. Ke2 {[ts=3.400]} 9B. e3 {[ts=3.500]} 10a. Q@d2 {[ts=3.600]}
                11A. Bxd2 {[ts=3.700]} 11a. Qxd2 {[ts=3.800]}
                "#
            )
        );
    }

    #[test]
    fn pgn_parse() {
        let source = indoc!(
            r#"
            [Event "Unrated Bughouse Match"]
            [Site "bughouse.pro"]
            [UTCDate "2024.03.12"]
            [UTCTime "21:48:55"]
            [Round "6"]
            [WhiteA "Санёк"]
            [BlackA "Andrei"]
            [WhiteB "km"]
            [BlackB "Санёк"]
            [TimeControl "300"]
            [Variant "Bughouse Chess960"]
            [Promotion "Steal"]
            [DropAggression "No chess mate"]
            [PawnDropRanks "2-6"]
            [SetUp "1"]
            [FEN "nrbbqnkr/pppppppp/8/8/8/8/PPPPPPPP/NRBBQNKR w BHbh - 0 1 | nrbbqnkr/pppppppp/8/8/8/8/PPPPPPPP/NRBBQNKR w BHbh - 0 1"]
            [Result "0-1"]
            [Termination "normal"]
            [Outcome "Andrei & km won: Санёк & Санёк checkmated"]
            1A. d4 {[ts=0.000]} 1B. Ne3 {[ts=0.861]} 1b. d5 {[ts=1.419]} 1a. d5 {[ts=1.495]}
            2B. Nxd5 {[ts=2.869]} 2A. Ne3 {[ts=3.355]} 2a. Nb6 {[ts=4.711]}
            2b. c6 {[ts=7.610]} 3B. Ne3 {[ts=9.853]} 3A. Bd2 {[ts=11.170]}
            3a. Ng6 {[ts=12.318]} 3b. Bb6 {[ts=13.718]} 4B. O-O {[ts=15.236]}
            4A. Nb3 {[ts=17.881]} 4a. P@h3 {[ts=18.722]} 4b. Ne6 {[ts=20.046]}
            5B. c3 {[ts=21.840]} 5A. Qf1 {[ts=25.237]} 5a. xg2 {[ts=28.353]}
            6A. Nxg2 {[ts=28.353]} 6a. Bh3 {[ts=30.573]} 5b. Bd7 {[ts=34.688]}
            6B. P@g3 {[ts=36.515]} 7A. f3 {[ts=51.487]} 6b. Qc8 {[ts=54.084]}
            7B. Bb3 {[ts=58.807]} 7b. P@d5 {[ts=62.965]} 8B. Bxd5 {[ts=67.762]}
            8b. xd5 {[ts=68.732]} 9B. Nxd5 {[ts=69.490]} 7a. P@e4 {[ts=71.529]}
            9b. Qc5 {[ts=72.826]} 10B. Nxb6 {[ts=76.506]} 10b. Nxb6 {[ts=82.742]}
            11B. b4 {[ts=84.226]} 11b. Qc4 {[ts=87.638]} 8A. B@g4 {[ts=93.399]}
            8a. Bxg4 {[ts=98.726]} 9A. xg4 {[ts=99.636]} 12B. B@b3 {[ts=100.061]}
            12b. Qe4 {[ts=103.163]} 13B. d3 {[ts=107.272]} 9a. Nc4 {[ts=109.294]}
            13b. Qg6 {[ts=114.639]} 14B. Nc2 {[ts=125.669]} 10A. e3 {[ts=135.755]}
            10a. Nxd2 {[ts=137.840]} 14b. O-O-O {[ts=143.159]} 11A. Nxd2 {[ts=145.463]}
            15B. a4 {[ts=147.139]} 15b. Ng5 {[ts=153.959]} 11a. c6 {[ts=155.412]}
            16B. a5 {[ts=162.322]} 12A. Qf2 {[ts=165.612]} 16b. Na8 {[ts=169.582]}
            17B. Bxg5 {[ts=173.190]} 17b. Qxg5 {[ts=175.876]} 12a. Bc7 {[ts=176.112]}
            18B. B@e3 {[ts=176.827]} 18b. Qf6 {[ts=193.519]} 19B. Bxa7 {[ts=198.879]}
            13A. B@g3 {[ts=200.341]} 13a. P@h3 {[ts=209.432]} 14A. Nf4 {[ts=213.453]}
            14a. Bxf4 {[ts=221.712]} 15A. xf4 {[ts=221.712]} 19b. Bc6 {[ts=229.359]}
            20B. Bb6 {[ts=235.154]} 20b. Nxb6 {[ts=238.723]} 21B. xb6 {[ts=241.222]}
            21b. Kd7 {[ts=242.080]} 22B. N@c5 {[ts=253.766]} 22b. Ke8 {[ts=257.339]}
            23B. Ra1 {[ts=258.317]} 23b. B@a6 {[ts=268.748]} 24B. Nxa6 {[ts=271.232]}
            24b. xa6 {[ts=271.997]} 15a. P@f3 {[ts=272.820]} 25B. Rxa6 {[ts=273.261]}
            25b. Bb5 {[ts=276.053]} 26B. Ra7 {[ts=282.106]} 16A. Bxf3 {[ts=282.755]}
            16a. xf3 {[ts=284.871]} 17A. Nxf3 {[ts=284.871]} 26b. Qxb6 {[ts=287.331]}
            17a. B@e4 {[ts=289.356]} 27B. B@c5 {[ts=294.309]} 18A. O-O-O {[ts=302.639]}
            18a. Bxc2 {[ts=309.442]} 27b. Qxa7 {[ts=313.217]} 19A. Qxc2 {[ts=314.640]}
            28B. Bxa7 {[ts=315.428]} 28b. Bxd3 {[ts=318.291]} 29B. xd3 {[ts=321.598]}
            19a. P@e4 {[ts=326.632]} 29b. P@c7 {[ts=332.377]} 20A. Ng5 {[ts=338.512]}
            20a. B@d3 {[ts=342.375]} 30B. Ba4 {[ts=343.568]} 21A. Rxd3 {[ts=344.223]}
            21a. xd3 {[ts=345.337]} 22A. Qxd3 {[ts=345.337]} 22a. B@e4 {[ts=348.426]}
            30b. P@d7 {[ts=348.737]} 23A. Nxe4 {[ts=350.916]} 23a. xe4 {[ts=352.066]}
            24A. Qxe4 {[ts=353.136]} 31B. N@d5 {[ts=354.516]} 31b. B@d6 {[ts=359.919]}
            32B. P@c5 {[ts=362.544]} 32b. Ra8 {[ts=378.551]} 33B. xd6 {[ts=382.175]}
            24a. B@g2 {[ts=384.951]} 33b. cxd6 {[ts=387.394]} 34B. Qxe7 {[ts=390.375]}
            "#
        );
        let game = import_from_bpgn(source, Role::ServerOrStandalone).unwrap();
        let serialized = export_to_bpgn(BpgnExportFormat::default(), &game, UtcDateTime::now(), 1);
        let game2 = import_from_bpgn(&serialized, Role::ServerOrStandalone).unwrap();
        assert_eq!(game, game2);
    }

    // TODO: Test game without timestamps when it's supported.
    // TODO: Tests with more game variants.
}
