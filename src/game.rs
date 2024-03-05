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

use std::rc::Rc;

use enum_map::{enum_map, Enum, EnumMap};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

use crate::algebraic::{AlgebraicDetails, AlgebraicTurn};
use crate::board::{
    Board, ChessGameStatus, DrawReason, Reserve, Turn, TurnError, TurnExpanded, TurnFacts,
    TurnInput, TurnMode, VictoryReason,
};
use crate::clock::GameInstant;
use crate::coord::BoardShape;
use crate::force::Force;
use crate::piece::PieceKind;
use crate::player::Team;
use crate::role::Role;
use crate::rules::{BughouseRules, ChessRules, MatchRules, Rules};
use crate::starter::{generate_starting_position, EffectiveStartingPosition};


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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TurnIndex(pub String);

#[derive(Clone, Debug)]
pub struct TurnRecordExpanded {
    pub number: u32,
    pub mode: TurnMode,
    pub envoy: BughouseEnvoy,
    pub turn_expanded: TurnExpanded,
    pub time: GameInstant,
    pub board_after: Board,
}

impl TurnRecordExpanded {
    pub fn trim_for_sending(&self) -> TurnRecord {
        // This is used only to send confirmed turns from server to clients, so preturns
        // should never occur here.
        assert_eq!(self.mode, TurnMode::Normal);
        TurnRecord {
            envoy: self.envoy,
            turn_input: TurnInput::Explicit(self.turn_expanded.turn),
            time: self.time,
        }
    }

    // Improvement potential: Can we simply use the index in the `turn_log`?
    // Lexicographic order of indices is guaranteed to correspond to turn order.
    pub fn index(&self) -> TurnIndex {
        // Note. Black suffix should be lexicographically greater than white suffix.
        // Note. Not using "a"/"b" because it could be confused with board index.
        let force = match self.envoy.force {
            Force::White => "w",
            Force::Black => "x",
        };
        let id_duck_turn = matches!(self.turn_expanded.turn, Turn::PlaceDuck(_));
        let duck_suffix = if id_duck_turn { "d" } else { "" };
        TurnIndex(format!("{:08}-{}{}", self.number, force, duck_suffix))
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
        let board = Board::new(Rc::new(rules), role, player_names, &starting_position);
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
    pub fn to_readable_string(&self, rules: &ChessRules) -> String {
        use BughouseGameStatus::*;
        use DrawReason::*;
        use VictoryReason::*;
        let winners = self.winners.join(" & ");
        let losers = self.losers.join(" & ");
        match self.status {
            Active => "Unterminated".to_owned(),
            Victory(_, Checkmate) => {
                if rules.bughouse_rules.as_ref().map_or(false, |r| r.koedem) {
                    format!("{winners} won: {losers} lost all kings")
                } else if rules.regicide() {
                    format!("{winners} won: {losers} lost a king")
                } else {
                    format!("{winners} won: {losers} checkmated")
                }
            }
            Victory(_, Flag) => format!("{winners} won: {losers} lost on time"),
            Victory(_, Resignation) => format!("{winners} won: {losers} resigned"),
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
        }
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
    Observer,
}

// Player in an active game.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerInGame {
    pub name: String,
    pub id: BughousePlayer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
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
    pub fn is_player(self) -> bool { self.as_player().is_some() }
    pub fn is_observer(self) -> bool { !self.is_player() }
    pub fn as_player(self) -> Option<BughousePlayer> {
        match self {
            BughouseParticipant::Player(player) => Some(player),
            BughouseParticipant::Observer => None,
        }
    }
    pub fn envoy_for(self, board_idx: BughouseBoard) -> Option<BughouseEnvoy> {
        self.as_player().and_then(|p| p.envoy_for(board_idx))
    }
    pub fn plays_on_board(self, board_idx: BughouseBoard) -> bool {
        self.as_player().map_or(false, |p| p.plays_on_board(board_idx))
    }
    pub fn plays_for(self, envoy: BughouseEnvoy) -> bool {
        self.as_player().map_or(false, |p| p.plays_for(envoy))
    }
    pub fn envoys(self) -> Vec<BughouseEnvoy> { self.as_player().map_or(vec![], |p| p.envoys()) }
}


#[derive(Clone, Debug)]
pub struct BughouseGame {
    rules: Rc<Rules>,
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
        Self::new_with_starting_position_and_rules_rc(
            Rc::new(rules),
            role,
            starting_position,
            players,
        )
    }

    fn new_with_starting_position_and_rules_rc(
        rules: Rc<Rules>, role: Role, starting_position: EffectiveStartingPosition,
        players: &[PlayerInGame],
    ) -> Self {
        let player_map = make_player_map(players);
        let boards = if let EffectiveStartingPosition::ManualSetup(setup) = &starting_position {
            player_map.map(|board_idx, board_players| {
                Board::new_from_setup(
                    Rc::clone(&rules),
                    role,
                    board_players,
                    setup[&board_idx].clone(),
                )
            })
        } else {
            player_map.map(|_, board_players| {
                Board::new(Rc::clone(&rules), role, board_players, &starting_position)
            })
        };
        BughouseGame {
            rules,
            role,
            starting_position,
            boards,
            status: BughouseGameStatus::Active,
            turn_log: Vec::new(),
        }
    }

    pub fn clone_from_start(&self) -> Self {
        Self::new_with_starting_position_and_rules_rc(
            Rc::clone(&self.rules),
            self.role,
            self.starting_position.clone(),
            &self.players(),
        )
    }

    pub fn starting_position(&self) -> &EffectiveStartingPosition { &self.starting_position }
    pub fn rules(&self) -> &Rules { &self.rules }
    pub fn match_rules(&self) -> &MatchRules { &self.rules.match_rules }
    pub fn chess_rules(&self) -> &ChessRules { &self.rules.chess_rules }
    pub fn bughouse_rules(&self) -> &BughouseRules {
        self.rules.chess_rules.bughouse_rules.as_ref().unwrap()
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
    pub fn last_turn_record(&self) -> Option<&TurnRecordExpanded> { self.turn_log.last() }
    pub fn started(&self) -> bool { !self.turn_log.is_empty() }
    pub fn status(&self) -> BughouseGameStatus { self.status }
    pub fn is_active(&self) -> bool { self.status.is_active() }

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
                TurnMode::Normal
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
        let now = BughouseBoard::iter()
            .filter_map(|board| self.boards[board].flag_defeat_moment(now))
            .min();
        let Some(game_over_time) = now else {
            return None;
        };
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
        let is_duck_turn = board.is_duck_turn(envoy.force);
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

        // Changes to the board have been made. The function must not fail from this point on!
        //
        // An alternative solution would be clone the game state (like we do in `Board::try_turn`)
        // and through away the copy if anything went wrong. As a bonus, this would allow to
        // simplify the abovementioned `Board::try_turn`, because it would be able to freely mutate
        // `Board` state. The problem with this solution that cloning `Board::position_count` on
        // every turn would make application N turns take O(N^2) time. By the way, this is already
        // the case for the client because `AlteredGame::local_game` copied the entire game state,
        // but:
        //   - If needed, it can be fixed by stripping away `position_count` entirely. The client is
        //     not authorized to conclude that the game has ended anyway.
        //   - If one client becomes slow when the users decided to play for a thosand turns, this
        //     is not a disaster. If the entire server become irresponsive because of one such
        //     match, this is much worse.

        // If `try_turn` succeeded, then the turn was valid. Thus conversion to algebraic must
        // have succeeded as well, because there exists an algebraic form for any valid turn.
        let turn_algebraic = turn_algebraic.unwrap();
        let other_board = &mut self.boards[board_idx.other()];
        match mode {
            TurnMode::Normal => other_board.start_clock(now),
            TurnMode::Preturn => {}
        }
        other_board.apply_sibling_turn(&turn_facts, mode);

        let prev_number = self
            .turn_log
            .iter()
            .rev()
            .find(|record| record.envoy.board_idx == board_idx)
            .map_or(0, |record| record.number);
        let inc_number =
            (envoy.force == Force::White || mode == TurnMode::Preturn) && !is_duck_turn;
        let number = if inc_number { prev_number + 1 } else { prev_number };
        let turn_expanded = make_turn_expanded(turn, turn_algebraic, turn_facts);
        self.turn_log.push(TurnRecordExpanded {
            number,
            mode,
            envoy,
            turn_expanded,
            time: now,
            // Improvement potential: Show reserve prior to the next turn rather than after this one.
            board_after: self.boards[board_idx].clone_for_wayback(),
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
