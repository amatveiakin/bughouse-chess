// On terminology:
//   Participant: Somebody who is either playing or observing the game. There is
//     a 1:1 corresponence between participants and humans connected to a match.
//   Player: A participant who is playing (not observing) in a given game. Normally
//     there are 4 player in a game. However it is also possible to have a game with
//     2 or 3 players if some of them are double-playing, i.e. if they play on both
//     boards for a given team.
//   Observer: A participant who is not playing in a given game. An observer
//     could be temporary (if they were randomly selected to skip one game, but will
//     play next time) or permanent (if they never play in a given match).
//   Envoy: Representative of all pieces of a given color on a given board. There are
//     always exactly 4 envoys: (2 per team) x (2 per side). Each player controls one
//     or two envoys.
//
// Correspondence. How many <Column> objects there exist per one <Row> object:
//
//                   Human    Participant    Player     Envoy
//   Human                         1           0-1       0-2
//   Participant       1                       0-1       0-2
//   Player            1           1                     1-2
//   Envoy             1           1            1

// Improvement potential: Factor out defines for bughouse, leave only `Game` classes.
//   Or split into `chess.rs` and `bughouse.rs`.

#![allow(unused_parens)]

use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::hash::Hash;
use std::mem;
use std::str::FromStr;

use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

use crate::algebraic::{AlgebraicDetails, AlgebraicTurn};
use crate::board::{
    Board, ChessGameStatus, DrawReason, Reserve, Turn, TurnError, TurnExpanded, TurnFacts,
    TurnInput, TurnMode, VictoryReason,
};
use crate::clock::{GameDuration, GameInstant, MillisDuration};
use crate::coord::BoardShape;
use crate::force::Force;
use crate::once_cell_regex;
use crate::piece::PieceKind;
use crate::player::Team;
use crate::role::Role;
use crate::rules::{BughouseRules, ChessRules, MatchRules, Rules};
use crate::starter::{EffectiveStartingPosition, generate_starting_position};


pub const MIN_PLAYERS: usize = TOTAL_TEAMS;
pub const TOTAL_TEAMS: usize = 2;
pub const TOTAL_ENVOYS_PER_TEAM: usize = 2;
pub const TOTAL_ENVOYS: usize = TOTAL_TEAMS * TOTAL_ENVOYS_PER_TEAM;

// All information required in order to replay a turn.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnRecord {
    pub envoy: BughouseEnvoy,
    pub turn_input: TurnInput,
    pub time: GameInstant,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnIndex(pub usize);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TurnRecordExpanded {
    pub index: TurnIndex,  // global unique turn index
    pub local_number: u32, // logical turn number within the board
    pub mode: TurnMode,
    pub envoy: BughouseEnvoy,
    pub turn_expanded: TurnExpanded,
    pub time: GameInstant,
}

impl Display for TurnIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}
impl FromStr for TurnIndex {
    type Err = <usize as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> { Ok(TurnIndex(s.parse()?)) }
}

impl TurnRecordExpanded {
    pub fn trim(&self) -> TurnRecord {
        // This is used only to send confirmed turns from server to clients and replay wayback,
        // so preturns should never occur here.
        assert_eq!(self.mode, TurnMode::InOrder);
        TurnRecord {
            envoy: self.envoy,
            turn_input: TurnInput::Explicit(self.turn_expanded.turn),
            time: self.time,
        }
    }
}


#[derive(Clone, Debug)]
pub struct ChessGame {
    #[allow(dead_code)]
    starting_position: EffectiveStartingPosition,
    board: Board,
}

impl ChessGame {
    pub fn new(rules: Rules, role: Role, player_names: EnumMap<Force, String>) -> Self {
        let starting_position = generate_starting_position(&rules.chess_rules);
        Self::new_with_starting_position(rules, role, starting_position, player_names)
    }

    pub fn new_with_starting_position(
        rules: Rules, role: Role, starting_position: EffectiveStartingPosition,
        player_names: EnumMap<Force, String>,
    ) -> Self {
        assert!(rules.bughouse_rules().is_none());
        let board = Board::new(rules, role, player_names, &starting_position);
        ChessGame { starting_position, board }
    }

    pub fn rules(&self) -> &Rules { self.board.rules() }
    pub fn match_rules(&self) -> &MatchRules { &self.rules().match_rules }
    pub fn chess_rules(&self) -> &ChessRules { &self.rules().chess_rules }
    pub fn board(&self) -> &Board { &self.board }
    pub fn status(&self) -> ChessGameStatus { self.board.status() }

    pub fn test_flag(&mut self, now: GameInstant) { self.board.test_flag(now); }

    // Function from `try_turn...` family do not test flag internally. They will not update
    // game status if a player has zero time left.
    // Thus it's recommended to `test_flag` first.
    pub fn try_turn(
        &mut self, turn_input: &TurnInput, mode: TurnMode, now: GameInstant,
    ) -> Result<Turn, TurnError> {
        let turn = self.board.parse_turn_input(turn_input, mode, None)?;
        self.board.try_turn(turn, mode, now)?;
        Ok(turn)
    }
}


#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Enum, EnumIter, Serialize, Deserialize,
)]
pub enum BughouseBoard {
    A,
    B,
}

// Improvement potential. Consider whether "not started" should be a separate status.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BughouseGameStatus {
    Active,
    Victory(Team, VictoryReason),
    Draw(DrawReason),
}

// `winner` and `losers` arrays are non-empty iff `status` is `Victory`.
// `winner` and `losers` are ordered by the board, not by name.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameOutcome {
    pub status: BughouseGameStatus,
    pub winners: Vec<String>,
    pub losers: Vec<String>,
}

impl BughouseBoard {
    pub fn other(self) -> Self {
        match self {
            BughouseBoard::A => BughouseBoard::B,
            BughouseBoard::B => BughouseBoard::A,
        }
    }
}

impl BughouseGameStatus {
    pub fn is_active(&self) -> bool { *self == BughouseGameStatus::Active }
}

impl GameOutcome {
    pub fn to_pgn(&self, rules: &ChessRules) -> String {
        use BughouseGameStatus::*;
        use DrawReason::*;
        use VictoryReason::*;
        let winners = self.winners.join(" & ");
        let losers = self.losers.join(" & ");
        match self.status {
            Active => "Unterminated".to_owned(),
            Victory(_, Checkmate) => {
                if rules.bughouse_rules.as_ref().is_some_and(|r| r.koedem) {
                    format!("{winners} won: {losers} lost all kings")
                } else if rules.regicide() {
                    format!("{winners} won: {losers} lost a king")
                } else {
                    format!("{winners} won: {losers} checkmated")
                }
            }
            Victory(_, Flag) => format!("{winners} won: {losers} lost on time"),
            Victory(_, Resignation) => format!("{winners} won: {losers} resigned"),
            Victory(_, UnknownVictory) => format!("{winners} won, {losers} lost"),
            Draw(SimultaneousCheckmate) => {
                if rules.regicide() {
                    "Draw: both kings lost".to_owned()
                } else {
                    // Future-proofing: this isn't possible at the time of writing.
                    "Draw: both players checkmated".to_owned()
                }
            }
            Draw(SimultaneousFlag) => "Draw: simultaneous flags".to_owned(),
            Draw(ThreefoldRepetition) => "Draw: threefold repetition".to_owned(),
            Draw(UnknownDraw) => "Draw".to_owned(),
        }
    }

    pub fn from_pgn(players: &[PlayerInGame], s: &str) -> Result<Self, String> {
        use BughouseGameStatus::*;
        use DrawReason::*;
        use VictoryReason::*;
        let checkmate_victory_re =
            once_cell_regex!("^(.+) won: (.+) (?:lost all kings|lost a king|checkmated)$");
        let flag_victory_re = once_cell_regex!("^(.+) won: (.+) lost on time$");
        let resignation_victory_re = once_cell_regex!("^(.+) won: (.+) resigned$");
        let unknown_victory_re = once_cell_regex!("^(.+) won, (.+) lost$");
        let simultaneous_checkmate_draw_re =
            once_cell_regex!("^Draw: both kings lost|Draw: both players checkmated$");
        let simultaneous_flag_draw_re = once_cell_regex!("^Draw: simultaneous flags$");
        let threefold_repetition_draw_re = once_cell_regex!("^Draw: threefold repetition$");
        let unknown_draw_re = once_cell_regex!("^Draw$");

        if s == "Unterminated" {
            return Ok(GameOutcome {
                status: Active,
                winners: vec![],
                losers: vec![],
            });
        }
        for (regex, reason) in [
            (checkmate_victory_re, Checkmate),
            (flag_victory_re, Flag),
            (resignation_victory_re, Resignation),
            (unknown_victory_re, UnknownVictory),
        ] {
            if let Some(captures) = regex.captures(s) {
                let make_players =
                    |s: &str| s.split('&').map(|s| s.trim().to_owned()).collect::<HashSet<_>>();
                let winners = make_players(captures.get(1).unwrap().as_str());
                let losers = make_players(captures.get(2).unwrap().as_str());
                let mut teams = Self::get_teams(players);
                let winner_team = teams
                    .iter()
                    .find(|(_, team_players)| **team_players == winners)
                    .map(|(team, _)| team)
                    .ok_or_else(|| "winner set does not match player set".to_owned())?;
                let loser_team = winner_team.opponent();
                if teams[loser_team] != losers {
                    return Err("loser set does not match player set".to_owned());
                }
                return Ok(GameOutcome {
                    status: Victory(winner_team, reason),
                    winners: mem::take(&mut teams[winner_team]).into_iter().collect(),
                    losers: mem::take(&mut teams[loser_team]).into_iter().collect(),
                });
            }
        }
        for (regex, reason) in [
            (simultaneous_checkmate_draw_re, SimultaneousCheckmate),
            (simultaneous_flag_draw_re, SimultaneousFlag),
            (threefold_repetition_draw_re, ThreefoldRepetition),
            (unknown_draw_re, UnknownDraw),
        ] {
            if regex.is_match(s) {
                return Ok(GameOutcome {
                    status: Draw(reason),
                    winners: vec![],
                    losers: vec![],
                });
            }
        }
        Err(format!("unrecognized game outcome: \"{}\"", s))
    }

    pub fn from_legacy_pgn(
        players: &[PlayerInGame], s: &str,
    ) -> Result<BughouseGameStatus, String> {
        use BughouseGameStatus::*;
        use DrawReason::*;
        use VictoryReason::*;
        let checkmate_victory_re = once_cell_regex!("^(.+) won by checkmate$");
        let flag_victory_re = once_cell_regex!("^(.+) won by flag$");
        let resignation_victory_re = once_cell_regex!("^(.+) won by resignation$");
        let simultaneous_flag_draw_re = once_cell_regex!("^Draw by simultaneous flags$");
        let threefold_repetition_draw_re = once_cell_regex!("^Draw by threefold repetition$");

        if s == "Unterminated" {
            return Ok(Active);
        }
        for (regex, reason) in [
            (checkmate_victory_re, Checkmate),
            (flag_victory_re, Flag),
            (resignation_victory_re, Resignation),
        ] {
            if let Some(captures) = regex.captures(s) {
                let make_players =
                    |s: &str| s.split('&').map(|s| s.trim().to_owned()).collect::<HashSet<_>>();
                let winners = make_players(captures.get(1).unwrap().as_str());
                let teams = Self::get_teams(players);
                let winner_team = teams
                    .iter()
                    .find(|(_, team_players)| **team_players == winners)
                    .map(|(team, _)| team)
                    .ok_or_else(|| "winner set does not match player set".to_owned())?;
                return Ok(Victory(winner_team, reason));
            }
        }
        for (regex, reason) in [
            (simultaneous_flag_draw_re, SimultaneousFlag),
            (threefold_repetition_draw_re, ThreefoldRepetition),
        ] {
            if regex.is_match(s) {
                return Ok(Draw(reason));
            }
        }
        Err(format!("unrecognized legacy game outcome: \"{}\"", s))
    }

    pub fn to_readable_string(&self, rules: &ChessRules) -> String { self.to_pgn(rules) }

    fn get_teams(players: &[PlayerInGame]) -> EnumMap<Team, HashSet<String>> {
        let mut team_players = enum_map! { _ => HashSet::new() };
        for p in players {
            team_players[p.id.team()].insert(p.name.clone());
        }
        team_players
    }
}

pub fn get_bughouse_team(board_idx: BughouseBoard, force: Force) -> Team {
    match (board_idx, force) {
        (BughouseBoard::A, Force::White) | (BughouseBoard::B, Force::Black) => Team::Red,
        (BughouseBoard::B, Force::White) | (BughouseBoard::A, Force::Black) => Team::Blue,
    }
}
pub fn get_bughouse_board(team: Team, force: Force) -> BughouseBoard {
    match (team, force) {
        (Team::Red, Force::White) | (Team::Blue, Force::Black) => BughouseBoard::A,
        (Team::Blue, Force::White) | (Team::Red, Force::Black) => BughouseBoard::B,
    }
}
pub fn get_bughouse_force(team: Team, board_idx: BughouseBoard) -> Force {
    match (team, board_idx) {
        (Team::Red, BughouseBoard::A) | (Team::Blue, BughouseBoard::B) => Force::White,
        (Team::Blue, BughouseBoard::A) | (Team::Red, BughouseBoard::B) => Force::Black,
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct BughouseEnvoy {
    pub board_idx: BughouseBoard,
    pub force: Force,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum BughousePlayer {
    SinglePlayer(BughouseEnvoy),
    DoublePlayer(Team),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BughouseParticipant {
    Player(BughousePlayer),
    Observer(BughouseEnvoy),
}

// Player in an active game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerInGame {
    pub name: String,
    pub id: BughousePlayer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayerRelation {
    Myself,
    Opponent,
    Partner,
    Diagonal,
    Other,
}

impl BughouseEnvoy {
    pub fn iter() -> impl Iterator<Item = BughouseEnvoy> {
        BughouseBoard::iter().flat_map(|board_idx| {
            Force::iter().map(move |force| BughouseEnvoy { board_idx, force })
        })
    }

    pub fn team(self) -> Team { get_bughouse_team(self.board_idx, self.force) }
    pub fn opponent(self) -> Self {
        BughouseEnvoy {
            board_idx: self.board_idx,
            force: self.force.opponent(),
        }
    }
    pub fn partner(self) -> Self {
        BughouseEnvoy {
            board_idx: self.board_idx.other(),
            force: self.force.opponent(),
        }
    }
    pub fn diagonal(self) -> Self {
        BughouseEnvoy {
            board_idx: self.board_idx.other(),
            force: self.force,
        }
    }
}

impl BughousePlayer {
    pub fn is_single_player(self) -> bool { self.as_single_player().is_some() }
    pub fn is_double_player(self) -> bool { self.as_double_player().is_some() }
    pub fn as_single_player(self) -> Option<BughouseEnvoy> {
        match self {
            BughousePlayer::SinglePlayer(envoy) => Some(envoy),
            BughousePlayer::DoublePlayer(_) => None,
        }
    }
    pub fn as_double_player(self) -> Option<Team> {
        match self {
            BughousePlayer::SinglePlayer(_) => None,
            BughousePlayer::DoublePlayer(team) => Some(team),
        }
    }
    pub fn observe(self) -> BughouseParticipant {
        let envoy = match self {
            BughousePlayer::SinglePlayer(envoy) => envoy,
            BughousePlayer::DoublePlayer(team) => {
                let force = Force::White;
                let board_idx = get_bughouse_board(team, force);
                BughouseEnvoy { board_idx, force }
            }
        };
        BughouseParticipant::Observer(envoy)
    }

    pub fn team(self) -> Team {
        match self {
            BughousePlayer::SinglePlayer(envoy) => envoy.team(),
            BughousePlayer::DoublePlayer(team) => team,
        }
    }
    pub fn envoy_for(self, board_idx: BughouseBoard) -> Option<BughouseEnvoy> {
        match self {
            BughousePlayer::SinglePlayer(envoy) => (envoy.board_idx == board_idx).then_some(envoy),
            BughousePlayer::DoublePlayer(team) => Some(BughouseEnvoy {
                board_idx,
                force: get_bughouse_force(team, board_idx),
            }),
        }
    }
    pub fn envoys(self) -> Vec<BughouseEnvoy> {
        match self {
            BughousePlayer::SinglePlayer(envoy) => vec![envoy],
            BughousePlayer::DoublePlayer(team) => BughouseBoard::iter()
                .map(|board_idx| BughouseEnvoy {
                    board_idx,
                    force: get_bughouse_force(team, board_idx),
                })
                .collect(),
        }
    }
    pub fn plays_on_board(self, board_idx: BughouseBoard) -> bool {
        self.envoy_for(board_idx).is_some()
    }
    pub fn plays_for(self, envoy: BughouseEnvoy) -> bool {
        match self {
            BughousePlayer::SinglePlayer(e) => e == envoy,
            BughousePlayer::DoublePlayer(team) => envoy.team() == team,
        }
    }

    pub fn relation_to(self, other_player: BughousePlayer) -> PlayerRelation {
        use BughousePlayer::*;
        let common_board = match (self, other_player) {
            (SinglePlayer(p1), SinglePlayer(p2)) => p1.board_idx == p2.board_idx,
            (DoublePlayer(_), _) | (_, DoublePlayer(_)) => true,
        };
        let same_team = self.team() == other_player.team();
        match (common_board, same_team) {
            (true, true) => {
                if self == other_player {
                    PlayerRelation::Myself
                } else {
                    // Shouldn't normally happen. We would only get here if trying to
                    // compute relation between e.g. "a player who plays for the entire
                    // red team" and "a player who plays for board A in red team". In an
                    // actual game such two players cannot coexist.
                    PlayerRelation::Other
                }
            }
            (true, false) => PlayerRelation::Opponent,
            (false, true) => PlayerRelation::Partner,
            (false, false) => PlayerRelation::Diagonal,
        }
    }
}

impl BughouseParticipant {
    pub fn default_observer() -> Self {
        BughouseParticipant::Observer(BughouseEnvoy {
            board_idx: BughouseBoard::A,
            force: Force::White,
        })
    }
    pub fn is_player(self) -> bool { self.as_player().is_some() }
    pub fn is_observer(self) -> bool { !self.is_player() }
    pub fn as_player(self) -> Option<BughousePlayer> {
        match self {
            BughouseParticipant::Player(player) => Some(player),
            BughouseParticipant::Observer(_) => None,
        }
    }
    pub fn envoy_for(self, board_idx: BughouseBoard) -> Option<BughouseEnvoy> {
        self.as_player().and_then(|p| p.envoy_for(board_idx))
    }
    pub fn plays_on_board(self, board_idx: BughouseBoard) -> bool {
        self.as_player().is_some_and(|p| p.plays_on_board(board_idx))
    }
    pub fn plays_for(self, envoy: BughouseEnvoy) -> bool {
        self.as_player().is_some_and(|p| p.plays_for(envoy))
    }
    pub fn envoys(self) -> Vec<BughouseEnvoy> { self.as_player().map_or(vec![], |p| p.envoys()) }
}


#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BughouseGame {
    role: Role,
    starting_position: EffectiveStartingPosition,
    boards: EnumMap<BughouseBoard, Board>,
    turn_log: Vec<TurnRecordExpanded>,
    status: BughouseGameStatus,
}

// Improvement potential. Remove mutable access to fields.
impl BughouseGame {
    pub fn new(rules: Rules, role: Role, players: &[PlayerInGame]) -> Self {
        assert!(rules.bughouse_rules().is_some());
        let starting_position = generate_starting_position(&rules.chess_rules);
        Self::new_with_starting_position(rules, role, starting_position, players)
    }

    pub fn new_with_starting_position(
        rules: Rules, role: Role, starting_position: EffectiveStartingPosition,
        players: &[PlayerInGame],
    ) -> Self {
        let player_map = make_player_map(players);
        let boards = if let EffectiveStartingPosition::ManualSetup(setup) = &starting_position {
            player_map.map(|board_idx, board_players| {
                Board::new_from_setup(rules.clone(), role, board_players, setup[&board_idx].clone())
            })
        } else {
            player_map.map(|_, board_players| {
                Board::new(rules.clone(), role, board_players, &starting_position)
            })
        };
        BughouseGame {
            role,
            starting_position,
            boards,
            status: BughouseGameStatus::Active,
            turn_log: Vec::new(),
        }
    }

    pub fn clone_from_start(&self) -> Self {
        Self::new_with_starting_position(
            self.rules().clone(),
            self.role,
            self.starting_position.clone(),
            &self.players(),
        )
    }

    pub fn stub_players() -> Vec<PlayerInGame> {
        use BughouseBoard::*;
        use Force::*;
        let single_player = |name: &str, force, board_idx| PlayerInGame {
            name: name.to_owned(),
            id: BughousePlayer::SinglePlayer(BughouseEnvoy { board_idx, force }),
        };
        vec![
            single_player("WhiteA", White, A),
            single_player("BlackA", Black, A),
            single_player("WhiteB", White, B),
            single_player("BlackB", Black, B),
        ]
    }

    pub fn starting_position(&self) -> &EffectiveStartingPosition { &self.starting_position }
    pub fn rules(&self) -> &Rules { self.board(BughouseBoard::A).rules() }
    pub fn match_rules(&self) -> &MatchRules { &self.rules().match_rules }
    pub fn chess_rules(&self) -> &ChessRules { &self.rules().chess_rules }
    pub fn bughouse_rules(&self) -> &BughouseRules {
        self.rules().chess_rules.bughouse_rules.as_ref().unwrap()
    }
    pub fn board_shape(&self) -> BoardShape { self.chess_rules().board_shape() }
    pub fn board_mut(&mut self, idx: BughouseBoard) -> &mut Board { &mut self.boards[idx] }
    pub fn board(&self, idx: BughouseBoard) -> &Board { &self.boards[idx] }
    pub fn boards(&self) -> &EnumMap<BughouseBoard, Board> { &self.boards }
    pub fn reserve(&self, envoy: BughouseEnvoy) -> &Reserve {
        self.boards[envoy.board_idx].reserve(envoy.force)
    }
    pub fn turn_log(&self) -> &Vec<TurnRecordExpanded> { &self.turn_log }
    pub fn turn_log_mut(&mut self) -> &mut Vec<TurnRecordExpanded> { &mut self.turn_log }
    pub fn turn_record(&self, index: TurnIndex) -> &TurnRecordExpanded { &self.turn_log[index.0] }
    pub fn last_turn_record(&self) -> Option<&TurnRecordExpanded> { self.turn_log.last() }
    pub fn started(&self) -> bool { !self.turn_log.is_empty() }
    pub fn status(&self) -> BughouseGameStatus { self.status }
    pub fn is_active(&self) -> bool { self.status.is_active() }

    pub fn total_time_elapsed(&self) -> GameDuration {
        // Both clocks should return the same time after the game is over.
        // Otherwise choose the value from the board where the latest turn was made.
        BughouseBoard::iter()
            .map(|board_idx| self.board(board_idx).clock().total_time_elapsed())
            .filter_map(|d| MillisDuration::try_from(d).ok())
            .max()
            .map_or(GameDuration::UNKNOWN, Into::into)
    }

    pub fn players(&self) -> Vec<PlayerInGame> {
        let mut ret = vec![];
        for team in Team::iter() {
            let same_player = BughouseBoard::iter()
                .map(|board_idx| {
                    self.boards[board_idx].player_name(get_bughouse_force(team, board_idx))
                })
                .all_equal();
            if same_player {
                let board_idx = BughouseBoard::A;
                let force = get_bughouse_force(team, board_idx);
                ret.push(PlayerInGame {
                    name: self.boards[board_idx].player_name(force).to_owned(),
                    id: BughousePlayer::DoublePlayer(team),
                });
            } else {
                for board_idx in BughouseBoard::iter() {
                    let force = get_bughouse_force(team, board_idx);
                    ret.push(PlayerInGame {
                        name: self.boards[board_idx].player_name(force).to_owned(),
                        id: BughousePlayer::SinglePlayer(BughouseEnvoy { board_idx, force }),
                    });
                }
            }
        }
        ret
    }
    pub fn player_map(&self) -> HashMap<String, BughousePlayer> {
        self.players().into_iter().map(|p| (p.name, p.id)).collect()
    }
    pub fn find_player(&self, player_name: &str) -> Option<BughousePlayer> {
        // Improvement potential: Avoid constructing `players` vector.
        self.players().iter().find(|p| p.name == player_name).map(|p| p.id)
    }
    pub fn is_envoy_active(&self, envoy: BughouseEnvoy) -> bool {
        self.status.is_active() && self.boards[envoy.board_idx].active_force() == envoy.force
    }
    pub fn turn_mode_for_envoy(&self, envoy: BughouseEnvoy) -> Result<TurnMode, TurnError> {
        if self.status.is_active() {
            Ok(if self.is_envoy_active(envoy) {
                TurnMode::InOrder
            } else {
                TurnMode::Preturn
            })
        } else {
            Err(TurnError::GameOver)
        }
    }

    pub fn set_status(&mut self, status: BughouseGameStatus, now: GameInstant) {
        self.status = status;
        if !status.is_active() {
            for (_, board) in self.boards.iter_mut() {
                board.clock_mut().stop(now);
            }
        }
    }

    // Returns game over time, if any.
    pub fn test_flag(&mut self, now: GameInstant) -> Option<GameInstant> {
        use BughouseBoard::*;
        use BughouseGameStatus::*;
        use VictoryReason::Flag;
        assert_eq!(self.status, Active);
        let game_duration = BughouseBoard::iter()
            .filter_map(|board| self.boards[board].flag_defeat_moment(now))
            .filter_map(|d| MillisDuration::try_from(d.elapsed_since_start()).ok())
            .min()?;
        let game_over_time = GameInstant::from_millis_duration(game_duration);
        self.boards[A].test_flag(game_over_time);
        self.boards[B].test_flag(game_over_time);
        let status_a = self.game_status_for_board(A);
        let status_b = self.game_status_for_board(B);
        let status = match (status_a, status_b) {
            (Victory(winner_a, Flag), Victory(winner_b, Flag)) => {
                if winner_a == winner_b {
                    Victory(winner_a, Flag)
                } else {
                    Draw(DrawReason::SimultaneousFlag)
                }
            }
            (Victory(winner, Flag), Active) => Victory(winner, Flag),
            (Active, Victory(winner, Flag)) => Victory(winner, Flag),
            (Active, Active) => {
                panic!("Unexpected active status after `flag_defeat_moment` returned `Some`")
            }
            (Victory(_, reason), _) => {
                panic!("Unexpected victory reason in `test_flag`: {:?}", reason)
            }
            (_, Victory(_, reason)) => {
                panic!("Unexpected victory reason in `test_flag`: {:?}", reason)
            }
            (Draw(_), _) | (_, Draw(_)) => panic!("Unexpected draw in `test_flag`"),
        };
        self.set_status(status, game_over_time);
        Some(game_over_time)
    }

    // Should `test_flag` first!
    pub fn try_turn(
        &mut self, board_idx: BughouseBoard, turn_input: &TurnInput, mode: TurnMode,
        now: GameInstant,
    ) -> Result<Turn, TurnError> {
        if !self.status.is_active() {
            // `Board::try_turn` will also test game status, but that's not enough: the game
            // may have ended earlier on the other board.
            return Err(TurnError::GameOver);
        }
        let board = &self.boards[board_idx];
        let other_board = &self.boards[board_idx.other()];
        let envoy = BughouseEnvoy { board_idx, force: board.turn_owner(mode) };
        let turn = board.parse_turn_input(turn_input, mode, Some(other_board))?;
        let local_number = board.full_turn_index();
        self.boards[board_idx.other()].verify_sibling_turn(turn, mode, envoy.force)?;

        // `turn_to_algebraic` must be called before `try_turn`, because algebraic form depend
        // on the current position.
        let turn_algebraic = board.turn_to_algebraic(
            turn,
            mode,
            Some(other_board),
            AlgebraicDetails::ShortAlgebraic,
        );

        let turn_facts = self.boards[board_idx].try_turn(turn, mode, now)?;

        if self.bughouse_rules().duplicate {
            self.boards[board_idx].check_duplicate(&turn_facts);
        }

        // Changes to the board have been made. The function must not fail from this point on!
        //
        // An alternative solution would be clone the game state (like we do in `Board::try_turn`)
        // and through away the copy if anything went wrong. As a bonus, this would allow to
        // simplify the abovementioned `Board::try_turn`, because it would be able to freely mutate
        // `Board` state. The problem with this solution that cloning `Board::position_count` on
        // every turn would make application N turns take O(N^2) time.

        // If `try_turn` succeeded, then the turn was valid. Thus conversion to algebraic must
        // have succeeded as well, because there exists an algebraic form for any valid turn.
        let turn_algebraic = turn_algebraic.unwrap();
        let other_board = &mut self.boards[board_idx.other()];
        match mode {
            TurnMode::InOrder | TurnMode::Virtual => other_board.start_clock(now),
            TurnMode::Preturn => {}
        }
        other_board.apply_sibling_turn(&turn_facts, mode);

        let turn_expanded = make_turn_expanded(turn, turn_algebraic, turn_facts);
        self.turn_log.push(TurnRecordExpanded {
            index: TurnIndex(self.turn_log.len()),
            local_number,
            mode,
            envoy,
            turn_expanded,
            time: now,
        });
        assert!(self.status.is_active());
        if self.bughouse_rules().koedem {
            self.check_koedem_victory(now);
        } else {
            self.set_status(self.game_status_for_board(board_idx), now);
        }
        Ok(turn)
    }

    pub fn try_turn_by_envoy(
        &mut self, envoy: BughouseEnvoy, turn_input: &TurnInput, mode: TurnMode, now: GameInstant,
    ) -> Result<Turn, TurnError> {
        let expected_mode = self.turn_mode_for_envoy(envoy)?;
        if mode != expected_mode {
            return Err(TurnError::WrongTurnMode);
        }
        self.try_turn(envoy.board_idx, turn_input, mode, now)
    }

    pub fn apply_turn_record(
        &mut self, turn_record: &TurnRecord, mode: TurnMode,
    ) -> Result<Turn, TurnError> {
        self.try_turn_by_envoy(turn_record.envoy, &turn_record.turn_input, mode, turn_record.time)
    }

    pub fn check_koedem_victory(&mut self, now: GameInstant) {
        let mut num_kings = enum_map! { _ => 0 };
        for (board_idx, board) in &self.boards {
            for coord in board.shape().coords() {
                if let Some(piece) = board.grid()[coord] {
                    if piece.kind == PieceKind::King {
                        // Unwrap ok: King cannot be neutral.
                        let team = get_bughouse_team(board_idx, piece.force.try_into().unwrap());
                        num_kings[team] += 1;
                    }
                }
            }
            for force in Force::iter() {
                let team = get_bughouse_team(board_idx, force);
                num_kings[team] += board.reserve(force)[PieceKind::King];
            }
        }
        // Note. Could be less with preturns.
        assert!(num_kings.values().sum::<u8>() as usize <= TOTAL_ENVOYS);
        for team in Team::iter() {
            if num_kings[team] as usize == TOTAL_ENVOYS {
                self.set_status(BughouseGameStatus::Victory(team, VictoryReason::Checkmate), now);
            }
        }
    }

    pub fn outcome(&self) -> GameOutcome {
        use BughouseGameStatus::*;
        let team_players = |team| {
            // Note. Not using `self.players()` because the order there is not specified.
            BughouseBoard::iter()
                .map(|board_idx| {
                    self.board(board_idx)
                        .player_name(get_bughouse_force(team, board_idx))
                        .to_owned()
                })
                .collect_vec()
        };
        let status = self.status();
        let (winners, losers) = match status {
            Active => (vec![], vec![]),
            Victory(team, _) => (team_players(team), (team_players(team.opponent()))),
            Draw(_) => (vec![], vec![]),
        };
        GameOutcome { status, winners, losers }
    }

    fn game_status_for_board(&self, board_idx: BughouseBoard) -> BughouseGameStatus {
        match self.boards[board_idx].status() {
            ChessGameStatus::Active => BughouseGameStatus::Active,
            ChessGameStatus::Victory(force, reason) => {
                BughouseGameStatus::Victory(get_bughouse_team(board_idx, force), reason)
            }
            ChessGameStatus::Draw(reason) => BughouseGameStatus::Draw(reason),
        }
    }
}

fn make_player_map(players: &[PlayerInGame]) -> EnumMap<BughouseBoard, EnumMap<Force, String>> {
    let mut player_map: EnumMap<BughouseBoard, EnumMap<Force, Option<String>>> =
        enum_map! { _ => enum_map!{ _ => None } };
    let mut insert_player = |board_idx, force, name: &String| {
        let player_ref = &mut player_map[board_idx][force];
        assert!(player_ref.is_none());
        *player_ref = Some(name.clone());
    };
    for p in players {
        match p.id {
            BughousePlayer::SinglePlayer(envoy) => {
                insert_player(envoy.board_idx, envoy.force, &p.name);
            }
            BughousePlayer::DoublePlayer(team) => {
                for board_idx in BughouseBoard::iter() {
                    insert_player(board_idx, get_bughouse_force(team, board_idx), &p.name);
                }
            }
        }
    }
    player_map.map(|_, board_players| board_players.map(|_, p| p.unwrap()))
}

fn make_turn_expanded(turn: Turn, algebraic: AlgebraicTurn, facts: TurnFacts) -> TurnExpanded {
    let mut relocation = None;
    let mut relocation_extra = None;
    let mut drop = None;
    match turn {
        Turn::Move(mv) => {
            relocation = Some((mv.from, mv.to));
        }
        Turn::Drop(dr) => {
            drop = Some(dr.to);
        }
        Turn::Castle(_) => {
            let castling_relocations = facts.castling_relocations.unwrap();
            relocation = Some(castling_relocations.king);
            relocation_extra = Some(castling_relocations.rook);
        }
        Turn::PlaceDuck(to) => {
            drop = Some(to);
        }
    }
    TurnExpanded {
        turn,
        algebraic,
        relocation,
        relocation_extra,
        drop,
        captures: facts.captures,
        steals: facts.steals,
    }
}

// For tests.
pub fn single_player(name: &str, envoy: BughouseEnvoy) -> PlayerInGame {
    PlayerInGame {
        name: name.to_owned(),
        id: BughousePlayer::SinglePlayer(envoy),
    }
}

// For tests.
pub fn double_player(name: &str, team: Team) -> PlayerInGame {
    PlayerInGame {
        name: name.to_owned(),
        id: BughousePlayer::DoublePlayer(team),
    }
}

// For tests.
#[macro_export]
macro_rules! envoy {
    ($force:ident $board_idx:ident) => {
        $crate::game::BughouseEnvoy {
            board_idx: $crate::game::BughouseBoard::$board_idx,
            force: $crate::force::Force::$force,
        }
    };
}
