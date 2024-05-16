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
use crate::coord::BoardShape;
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


// Any information stored in BPGN not contained in the `BughouseGame` object.
#[derive(Clone, Copy, Debug)]
pub struct BpgnMetadata {
    pub game_start_time: UtcDateTime,
    pub round: u64,
}

// Other possible formats:
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
impl ReducedBughouseGameStatus {
    pub fn to_status_unknown_reason(self) -> BughouseGameStatus {
        use ReducedBughouseGameStatus::*;
        match self {
            Active => BughouseGameStatus::Active,
            Victory(team) => BughouseGameStatus::Victory(team, VictoryReason::UnknownVictory),
            Draw => BughouseGameStatus::Draw(DrawReason::UnknownDraw),
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
    fn get_and_parse<T, E>(
        &self, key: &str, parse: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<T, String> {
        match self.map.get(key) {
            None => Err(format!("missing {key} tag")),
            Some(tag) => parse(tag).map_err(|_| format!("cannot parse {key} tag")),
        }
    }
    fn get_and_parse_or<T, E>(
        &self, key: &str, parse: impl FnOnce(&str) -> Result<T, E>, default: T,
    ) -> Result<T, String> {
        match self.map.get(key) {
            None => Ok(default),
            Some(tag) => parse(tag).map_err(|_| format!("cannot parse {key} tag")),
        }
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

fn make_termination_string(status: BughouseGameStatus) -> Option<&'static str> {
    use BughouseGameStatus::*;
    use DrawReason::*;
    use VictoryReason::*;
    match status {
        Active => Some("unterminated"),
        Victory(_, Checkmate) => Some("normal"),
        Victory(_, Flag) => Some("time forfeit"),
        // There is no "resign" Termination, should use "normal" apparently:
        // https://lichess.org/forum/general-chess-discussion/how-do-i-make-it-say-that-one-side-resigned#4
        Victory(_, Resignation) => Some("normal"),
        Victory(_, UnknownVictory) => None,
        Draw(SimultaneousCheckmate) => Some("normal"),
        // Somehow I'm skeptical many chess engines would be prepared for a "time forfeit" draw
        Draw(SimultaneousFlag) => Some("normal"),
        Draw(ThreefoldRepetition) => Some("normal"),
        Draw(UnknownDraw) => None,
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
fn make_bughouse_bpng_header(game: &BughouseGame, meta: BpgnMetadata) -> BpgnHeader {
    use BughouseBoard::*;
    use Force::*;
    let now = time::OffsetDateTime::from(meta.game_start_time);
    let game_at_start = game.clone_from_start();
    let variants = iter::once("Bughouse")
        .chain(game.chess_rules().variants().into_iter().map(ChessVariant::to_pgn))
        .collect_vec();

    let mut h = BpgnHeader::new();
    h.push_tag("Event", make_event(game));
    h.push_tag("Site", "bughouse.pro");
    h.push_tag("UTCDate", now.format(format_description!("[year].[month].[day]")).unwrap());
    h.push_tag("UTCTime", now.format(format_description!("[hour]:[minute]:[second]")).unwrap());
    h.push_tag("Round", meta.round);
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
    if let Some(termination) = make_termination_string(game.status()) {
        h.push_tag("Termination", termination);
    }
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
pub fn export_to_bpgn(format: BpgnExportFormat, game: &BughouseGame, meta: BpgnMetadata) -> String {
    let header = make_bughouse_bpng_header(game, meta);
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

fn parse_meta(tags: &TagMap) -> Result<BpgnMetadata, String> {
    let date = tags.get_and_parse("UTCDate", |s| {
        time::Date::parse(s, format_description!("[year].[month].[day]"))
    })?;
    let time = tags.get_and_parse("UTCTime", |s| {
        time::Time::parse(s, format_description!("[hour]:[minute]:[second]"))
    })?;
    let game_start_time = date.with_time(time).into();
    let round = tags.get_and_parse("Round", str::parse)?;
    Ok(BpgnMetadata { game_start_time, round })
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
    let rated = tags.get("Event")?.starts_with("Rated");
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
    let board_shape = BoardShape::standard();
    // Defaults logic:
    //   - For Promotion: use Upgrade, because these are standard chess rules.
    //   - For PawnDropRanks and DropAggression: use the most permissive setting, so that games
    //     don't fail to parse.
    let promotion = tags.get_and_parse_or("Promotion", Promotion::from_pgn, Promotion::Upgrade)?;
    let pawn_drop_ranks = tags.get_and_parse_or(
        "PawnDropRanks",
        PawnDropRanks::from_pgn,
        PawnDropRanks::widest(board_shape),
    )?;
    let drop_aggression = tags.get_and_parse_or(
        "DropAggression",
        DropAggression::from_pgn,
        DropAggression::MateAllowed,
    )?;
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
    let result = parse_game_result(tags.get("Result")?)?;
    if let Ok(outcome_text) = tags.get("Outcome") {
        let mut status = GameOutcome::from_pgn(players, outcome_text).map(|o| o.status);
        // The logic is similar to `Result::or_else`, but returns the first error rather than the second
        // one.
        if !status.is_ok() {
            if let Ok(legacy_status) = GameOutcome::from_legacy_pgn(players, outcome_text) {
                status = Ok(legacy_status);
            }
        }
        let status = status?;
        if result != status.into() {
            return Err("inconsistent Outcome and Result tags".to_owned());
        }
        Ok(status)
    } else {
        Ok(result.to_status_unknown_reason())
    }
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

pub fn import_from_bpgn(s: &str, role: Role) -> Result<(BughouseGame, BpgnMetadata), String> {
    let doc = BpgnDocument::parse(s)?;
    let tags = TagMap {
        map: doc.header.tags.into_iter().collect(),
    };
    let meta = parse_meta(&tags)?;
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
    Ok((game, meta))
}


#[cfg(test)]
mod tests {
    use std::time::Duration;

    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use time::macros::datetime;

    use super::*;
    use crate::clock::TimeBreakdown;
    use crate::role::Role;
    use crate::rules::{ChessRules, MatchRules, Rules};
    use crate::test_util::{replay_bughouse_log, sample_bughouse_players};
    use crate::{game_d, game_t};

    fn default_meta() -> BpgnMetadata {
        BpgnMetadata {
            game_start_time: UtcDateTime::now(),
            round: 1,
        }
    }

    fn algebraic(algebraic: &str) -> TurnInput { TurnInput::Algebraic(algebraic.to_owned()) }

    #[test]
    fn pgn_standard_conformity() {
        let rules = Rules {
            match_rules: MatchRules::unrated(),
            chess_rules: ChessRules::bughouse_chess_com(),
        };
        let mut game =
            BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
        replay_bughouse_log(
            &mut game,
            "1A.e4 1a.e5 1A.Nf3 1a.Nc6 1A.g3 1a.d5 1A.Bg2 1a.Qe7 1A.Nxe5 1a.xe4 1A.0-0",
            Duration::ZERO,
        )
        .unwrap();
        let bpgn = export_to_bpgn(BpgnExportFormat::default(), &game, default_meta());

        // Test: Uses short algebraic and includes capture notations.
        assert!(bpgn.contains(" Nx"));
        // Test: Does not contain non-ASCII characters (like "×").
        assert!(bpgn.chars().all(|ch| ch.is_ascii()));
        // Test: Castling is PGN-style (not FIDE-style).
        assert!(bpgn.contains("O-O"));
        assert!(!bpgn.contains("0-0"));
    }

    #[test]
    fn game_duration() {
        use BughouseBoard::*;
        use Force::*;
        let mut rules = Rules {
            match_rules: MatchRules::unrated(),
            chess_rules: ChessRules::bughouse_chess_com(),
        };
        rules.chess_rules.time_control.starting_time = Duration::from_secs(100);
        let mut game =
            BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
        game.try_turn(A, &algebraic("e4"), TurnMode::Normal, game_t!(0)).unwrap();
        game.try_turn(A, &algebraic("e5"), TurnMode::Normal, game_t!(10 s)).unwrap();
        game.try_turn(B, &algebraic("e4"), TurnMode::Normal, game_t!(20 s)).unwrap();
        game.test_flag(game_t!(999 s));

        let bpgn = export_to_bpgn(BpgnExportFormat::default(), &game, default_meta());
        let (game2, _) = import_from_bpgn(&bpgn, Role::ServerOrStandalone).unwrap();

        assert_eq!(game2.status(), BughouseGameStatus::Victory(Team::Blue, VictoryReason::Flag));
        let game_now = game_t!(0); // doesn't matter for finished games
        assert_eq!(game2.board(A).clock().time_left(Black, game_now), game_d!(90 s));
        assert_eq!(game2.board(A).clock().time_left(White, game_now), game_d!(0));
        assert_eq!(game2.board(A).clock().total_time_elapsed(), game_d!(110 s));
        let clock_showing = game2.board(A).clock().showing_for(White, game_now);
        assert_eq!(clock_showing.time_breakdown, TimeBreakdown::LowTime {
            seconds: 0,
            deciseconds: 0
        });
        assert!(clock_showing.out_of_time);
    }

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
        let meta = BpgnMetadata {
            game_start_time: UtcDateTime::from(datetime!(2024-03-06 13:37)),
            round: 1,
        };
        let bpgn = export_to_bpgn(BpgnExportFormat::default(), &game, meta);
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
        let source_old = indoc!(
            r#"
            [Event "Unrated Bughouse Match"]
            [Site "bughouse.pro"]
            [UTCDate "2023.03.01"]
            [UTCTime "19:33:14"]
            [Round "1"]
            [WhiteA "Санёк"]
            [BlackA "km"]
            [WhiteB "km"]
            [BlackB "Andrei"]
            [TimeControl "300"]
            [Variant "Bughouse Chess960"]
            [DropAggression "No chess mate"]
            [PawnDropRanks "2-6"]
            [SetUp "1"]
            [FEN "nrbbnqkr/pppppppp/8/8/8/8/PPPPPPPP/NRBBNQKR w BHbh - 0 1 | nrbbnqkr/pppppppp/8/8/8/8/PPPPPPPP/NRBBNQKR w BHbh - 0 1"]
            [Result "1-0"]
            [Termination "normal"]
            [Outcome "Санёк & Andrei won by checkmate"]
            1A. e4 1B. e4 1b. e5 1a. b6 2B. Be2 2b. Bg5 2A. c4 2a. Bb7 3B. Nf3 3A. d3
            3b. Bf6 4B. Nb3 3a. f5 4b. Nd6 5B. Nc5 5b. b6 4A. Bb3 4a. xe4 5A. c5 5a. e6
            6A. xb6 6B. Nd3 6a. Nxb6 6b. Nxe4 7A. xe4 7B. Nfxe5 7a. P@d5 7b. Bxe5 8B. Nxe5
            8b. Qe7 9B. d4 8A. xd5 8a. Bxd5 9b. P@f6 9A. P@c4 10B. Bd3 10b. Ng5 9a. Ba8
            10A. Nac2 11B. Bxg5 11b. xg5 10a. N@g5 11A. Bxg5 11a. Bxg5 12B. Qe2 12A. c5
            12b. P@f6 12a. Nd5 13B. Nxf7 13b. Qxe2 14B. Bxe2 14b. Kxf7 15B. Bc4 13A. B@f3
            15b. P@e6 13a. N8f6 16B. P@f5 16b. Re8 17B. O-O 17b. Kg8 14A. N@e5 18B. xe6
            18b. xe6 14a. P@d6 15A. Bbxd5 15a. Bxd5 16A. Bxd5 16a. Nxd5 17A. Nxd7 19B. P@f5
            17a. Qe7 19b. N@d2 18A. Nxb8 18a. O-O 20B. xe6 20b. Nxc4 19A. Nc6 21B. B@d5
            21b. Bxe6 22B. Bxe6 22b. Rxe6 23B. B@d5 19a. Qd7 23b. B@f7 24B. Bxc4 20A. N2d4
            24b. Re7 25B. Bxf7 25b. Rxf7 20a. B@d2 26B. P@e6 26b. Re7 27B. B@f7 27b. Kh8
            28B. Rfe1 28b. Rf8 21A. xd6 21a. Bxe1 22A. xc7 22a. P@h3 29B. N@f5 29b. R7xf7
            30B. xf7 30b. Rxf7 31B. Re8 23A. Rxe1 31b. R@f8 32B. R1e1 32b. P@h3 23a. xg2
            24A. Qxg2 24a. B@f4 33B. Rxf8 33b. Rxf8 25A. P@g3 34B. Ng3 34b. xg2 25a. Bxc7
            35B. Kxg2 35b. P@f3 36B. Kxf3 26A. Rxe6 26a. R@e1 27A. Rxe1 36b. g4 27a. Q@d2
            37B. Kxg4 28A. O-O 28a. Qxc6 29A. Nxc6 29a. N@e2 37b. h5 30A. Rxe2 30a. Qxe2
            38B. Nxh5 38b. f5 31A. Qxd5 31a. B@e6 39B. Kg5 32A. Qxg5 39b. Q@h6 40B. Kh4
            40b. R@g4 41B. Kh3 41b. Qxh5 32a. Bc4 42B. N@h4 33A. Ne7 42b. Qxh4
        "#
        );
        let source_regular = indoc!(
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
            [GameDuration "390.375"]
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
        let source_duck_accolade = indoc!(
            r#"
            [Event "Rated Bughouse Match"]
            [Site "bughouse.pro"]
            [UTCDate "2024.03.18"]
            [UTCTime "21:01:37"]
            [Round "5"]
            [WhiteA "Санёк"]
            [BlackA "Andrei"]
            [WhiteB "km"]
            [BlackB "Alex"]
            [TimeControl "300"]
            [Variant "Bughouse Accolade Chess960 DuckChess"]
            [Promotion "Steal"]
            [DropAggression "Mate allowed"]
            [PawnDropRanks "2-6"]
            [SetUp "1"]
            [FEN "nnrbbqkr/pppppppp/8/8/8/8/PPPPPPPP/NNRBBQKR w CHch - 0 1 | nnrbbqkr/pppppppp/8/8/8/8/PPPPPPPP/NNRBBQKR w CHch - 0 1"]
            [Result "0-1"]
            [Termination "normal"]
            [Outcome "Andrei & km won: Санёк & Alex lost a king"]
            [GameDuration "505.827"]
            1B. e4 {[ts=0.000]} 1B. @e6 {[ts=0.500]} 1A. Rb1 {[ts=1.505]}
            1A. @e6 {[ts=2.821]} 1b. d6 {[ts=3.359]} 1a. d5 {[ts=4.380]}
            1b. @e2 {[ts=4.803]} 1a. @d3 {[ts=5.662]} 2B. d4 {[ts=7.992]}
            2B. @e5 {[ts=8.471]} 2b. Bb5 {[ts=11.518]} 2b. @c4 {[ts=12.799]}
            2A. e3 {[ts=14.519]} 2A. @e6 {[ts=14.934]} 3B. Qe2 {[ts=15.735]}
            3B. @d3 {[ts=16.118]} 2a. Nc6 {[ts=17.790]} 2a. @e2 {[ts=18.315]}
            3b. e5 {[ts=20.090]} 3b. @c4 {[ts=21.787]} 3A. d4 {[ts=23.624]}
            3A. @b4 {[ts=24.041]} 3a. Nb6 {[ts=28.441]} 3a. @e2 {[ts=28.981]}
            4B. d5 {[ts=31.080]} 4B. @d2 {[ts=34.692]} 4A. Nb3 {[ts=35.446]}
            4A. @c4 {[ts=35.826]} 4b. Bxe2 {[ts=37.375]} 4a. Bc6 {[ts=38.911]}
            4a. @e2 {[ts=39.488]} 4b. @e7 {[ts=42.676]} 5B. Bxe2 {[ts=44.193]}
            5B. @b6 {[ts=46.102]} 5b. Bg5 {[ts=48.691]} 5b. @f4 {[ts=49.712]}
            6B. O-O {[ts=51.663]} 6B. @e3 {[ts=52.256]} 5A. Bd2 {[ts=57.576]}
            5A. @e1 {[ts=58.134]} 6b. Nb6 {[ts=60.838]} 5a. Nc4 {[ts=61.426]}
            5a. @e2 {[ts=61.898]} 6b. @d2 {[ts=62.573]} 7B. Rb1 {[ts=66.388]}
            7B. @d7 {[ts=68.596]} 7b. h5 {[ts=80.353]} 7b. @f3 {[ts=81.417]}
            8B. Ea3 {[ts=83.046]} 8B. @h4 {[ts=84.242]} 8b. a6 {[ts=95.655]}
            8b. @f3 {[ts=96.828]} 6A. c3 {[ts=98.558]} 6A. @d3 {[ts=99.595]}
            9B. Nb3 {[ts=113.332]} 9B. @g4 {[ts=114.354]} 9b. N8d7 {[ts=122.088]}
            9b. @f3 {[ts=123.139]} 10B. Nd2 {[ts=125.771]} 10B. @f4 {[ts=126.182]}
            10b. h4 {[ts=137.939]} 10b. @f3 {[ts=138.551]} 6a. e5 {[ts=140.818]}
            6a. @e2 {[ts=141.910]} 11B. Bd2 {[ts=145.990]} 11B. @f4 {[ts=146.476]}
            7A. Bc2 {[ts=150.602]} 7A. @d3 {[ts=151.800]} 11b. Nc5 {[ts=153.406]}
            11b. @e3 {[ts=154.516]} 7a. e4 {[ts=159.227]} 7a. @e2 {[ts=159.606]}
            12B. Cf3 {[ts=163.930]} 12B. @f4 {[ts=164.673]} 8A. Nc5 {[ts=173.061]}
            8A. @d3 {[ts=173.490]} 12b. f6 {[ts=175.740]} 12b. @g4 {[ts=180.723]}
            8a. Qe7 {[ts=187.596]} 8a. @d7 {[ts=188.141]} 13B. b4 {[ts=192.970]}
            13B. @d7 {[ts=193.553]} 9A. Ba4 {[ts=199.988]} 9A. @b5 {[ts=200.364]}
            13b. N5a4 {[ts=212.658]} 13b. @g4 {[ts=216.419]} 9a. B@d3 {[ts=221.181]}
            9a. @d7 {[ts=221.727]} 14B. c4 {[ts=228.051]} 14B. @h3 {[ts=231.791]}
            10A. Bxc6 {[ts=243.063]} 10A. @e2 {[ts=243.962]} 10a. Bxb1 {[ts=247.588]}
            10a. @d7 {[ts=248.488]} 14b. N@f4 {[ts=249.742]} 14b. @g4 {[ts=250.683]}
            11A. Bxd5 {[ts=252.713]} 11A. @d3 {[ts=253.139]} 11a. Nxd2 {[ts=257.338]}
            11a. @e6 {[ts=257.930]} 15B. B@e6 {[ts=260.131]} 15B. @f7 {[ts=260.552]}
            15b. Nxe6 {[ts=268.086]} 15b. @g4 {[ts=269.272]} 12A. Qd1 {[ts=270.638]}
            12A. @d3 {[ts=271.019]} 16B. xe6 {[ts=274.128]} 16B. @h3 {[ts=274.930]}
            16b. B@h5 {[ts=282.728]} 16b. @g4 {[ts=283.312]} 17B. Ce1 {[ts=297.969]}
            17B. @f3 {[ts=298.716]} 12a. N@f3 {[ts=301.402]} 12a. @e2 {[ts=301.814]}
            13A. xf3 {[ts=315.029]} 13A. @g2 {[ts=315.552]} 13a. Nxf3 {[ts=315.552]}
            13a. @e2 {[ts=315.552]} 17b. N@f4 {[ts=318.939]} 14A. Kf1 {[ts=320.317]}
            14A. @d2 {[ts=320.755]} 17b. @g4 {[ts=321.534]} 18B. N@c3 {[ts=338.906]}
            18B. @f3 {[ts=339.488]} 14a. Bd3 {[ts=341.295]} 14a. @e2 {[ts=341.663]}
            18b. Nxc3 {[ts=347.026]} 15A. Kg2 {[ts=349.069]} 15A. @e1 {[ts=349.466]}
            18b. @b3 {[ts=349.977]} 19B. Cxc3 {[ts=352.042]} 19B. @f3 {[ts=352.637]}
            19b. h3 {[ts=356.567]} 19b. @g4 {[ts=357.480]} 15a. N@h4 {[ts=365.102]}
            15a. @h3 {[ts=365.446]} 16A. Kg3 {[ts=390.036]} 16A. @e2 {[ts=391.371]}
            16a. Qg5 {[ts=396.467]} 16a. @g4 {[ts=396.844]} 17A. Kh3 {[ts=408.858]}
            17A. @g3 {[ts=409.228]} 20B. xh3 {[ts=416.108]} 20B. @f3 {[ts=416.624]}
            20b. Nxh3 {[ts=418.236]} 17a. Ng5 {[ts=418.876]} 20b. @h1 {[ts=419.398]}
            21B. Kg2 {[ts=421.967]} 17a. @g2 {[ts=422.297]} 21B. @f4 {[ts=422.431]}
            18A. Kg3 {[ts=425.537]} 18A. @g4 {[ts=425.876]} 18a. Nf5 {[ts=432.474]}
            18a. @g2 {[ts=432.976]} 21b. P@f3 {[ts=438.727]} 21b. @h1 {[ts=439.460]}
            22B. Bxf3 {[ts=441.773]} 22B. @g4 {[ts=442.135]} 22b. Nf4 {[ts=446.451]}
            22b. @h1 {[ts=447.570]} 19A. f4 {[ts=503.226]} 19A. @g4 {[ts=503.639]}
            19a. Nxg3 {[ts=505.827]}
            "#
        );
        // Test that legacy style BPGNs are still parsed. Output format is expected be different, so
        // compare game states instead.
        for source in [source_old] {
            let (game, meta) = import_from_bpgn(source, Role::ServerOrStandalone).unwrap();
            let serialized = export_to_bpgn(BpgnExportFormat::default(), &game, meta);
            let (game2, _) = import_from_bpgn(&serialized, Role::ServerOrStandalone).unwrap();
            assert_eq!(game, game2);
        }
        // Test that current style BPGNs are parsed. Output format is expected be identical.
        for source in [source_regular, source_duck_accolade] {
            let (game, meta) = import_from_bpgn(source, Role::ServerOrStandalone).unwrap();
            let serialized = export_to_bpgn(BpgnExportFormat::default(), &game, meta);
            assert_eq!(source, serialized);
        }
    }

    // TODO: Tests with more game variants.
}
