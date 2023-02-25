// On terminology:
//   Participant: Somebody who is either playing or observing the game. There is
//     a 1:1 corresponence between participants and humans connected to a contest.
//   Player: A participant who is playing (not observing) in a given game. Normally
//     there are 4 player in a game. However it is also possible to have a game with
//     2 or 3 players if some of them are double-playing, i.e. if they play on both
//     boards for a given team.
//   Observer: A participant who is not playing in a given game. An observer
//     could be temporary (if they were randomly selected to skip one game, but will
//     play next time) or permanent (if they never play in a given contest).
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

use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use serde::{Serialize, Deserialize};
use strum::{EnumIter, IntoEnumIterator};

use crate::board::{
    Board, Reserve, Turn, TurnInput, TurnExpanded, TurnFacts, TurnMode, TurnError,
    AlgebraicFormat, ChessGameStatus, VictoryReason, DrawReason
};
use crate::clock::GameInstant;
use crate::force::Force;
use crate::player::Team;
use crate::rules::{ContestRules, ChessRules, BughouseRules};
use crate::starter::{EffectiveStartingPosition, generate_starting_position};


pub const MIN_PLAYERS: usize = 2;
pub const TOTAL_ENVOYS: usize = 4;
pub const TOTAL_ENVOYS_PER_TEAM: usize = 2;

// Stripped version of `TurnRecordExpanded`. For sending turns across network.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnRecord {
    pub envoy: BughouseEnvoy,
    pub turn_algebraic: String,
    pub time: GameInstant,
}

#[derive(Clone, Debug)]
pub struct TurnRecordExpanded {
    pub mode: TurnMode,
    pub envoy: BughouseEnvoy,
    pub turn_expanded: TurnExpanded,
    pub time: GameInstant,
}

impl TurnRecordExpanded {
    pub fn trim_for_sending(&self) -> TurnRecord {
        // This is used only to send confirmed turns from server to clients, so preturns
        // should never occur here.
        assert_eq!(self.mode, TurnMode::Normal);
        TurnRecord {
            envoy: self.envoy,
            turn_algebraic: self.turn_expanded.algebraic_for_log.clone(),
            time: self.time,
        }
    }
}


#[derive(Clone, Debug)]
pub struct ChessGame {
    #[allow(dead_code)] starting_position: EffectiveStartingPosition,
    board: Board,
}

impl ChessGame {
    pub fn new(
        contest_rules: ContestRules,
        rules: ChessRules,
        player_names: EnumMap<Force, String>
    ) -> Self {
        let starting_position = generate_starting_position(rules.starting_position);
        Self::new_with_starting_position(contest_rules, rules, starting_position, player_names)
    }

    pub fn new_with_starting_position(
        contest_rules: ContestRules,
        rules: ChessRules,
        starting_position: EffectiveStartingPosition,
        player_names: EnumMap<Force, String>
    ) -> Self {
        let board = Board::new(
            Rc::new(contest_rules),
            Rc::new(rules),
            None,
            player_names,
            &starting_position,
        );
        ChessGame{ starting_position, board }
    }

    pub fn board(&self) -> &Board { &self.board }
    pub fn status(&self) -> ChessGameStatus { self.board.status() }

    pub fn test_flag(&mut self, now: GameInstant) {
        self.board.test_flag(now);
    }

    // Function from `try_turn...` familiy do not test flag internally. They will not update
    // game status if a player has zero time left.
    // Thus it's recommended to `test_flag` first.
    pub fn try_turn(&mut self, turn_input: &TurnInput, mode: TurnMode, now: GameInstant)
        -> Result<Turn, TurnError>
    {
        let turn = self.board.parse_turn_input(turn_input, mode)?;
        self.board.try_turn(turn, mode, now)?;
        Ok(turn)
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, EnumIter, Serialize, Deserialize)]
pub enum BughouseBoard {
    A,
    B,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BughouseGameStatus {
    Active,
    Victory(Team, VictoryReason),
    Draw(DrawReason),
}

impl BughouseBoard {
    pub fn other(self) -> Self {
        match self {
            BughouseBoard::A => BughouseBoard::B,
            BughouseBoard::B => BughouseBoard::A,
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
    pub fn team(self) -> Team {
        get_bughouse_team(self.board_idx, self.force)
    }
    pub fn opponent(self) -> Self {
        BughouseEnvoy {
            board_idx: self.board_idx,
            force: self.force.opponent(),
        }
    }
}

impl BughousePlayer {
    pub fn team(self) -> Team {
        match self {
            BughousePlayer::SinglePlayer(envoy) => envoy.team(),
            BughousePlayer::DoublePlayer(team) => team,
        }
    }
    pub fn envoy_for(self, board_idx: BughouseBoard) -> Option<BughouseEnvoy> {
        match self {
            BughousePlayer::SinglePlayer(envoy) => {
                if envoy.board_idx == board_idx { Some(envoy) } else { None }
            },
            BughousePlayer::DoublePlayer(team) => Some(BughouseEnvoy {
                board_idx,
                force: get_bughouse_force(team, board_idx),
            }),
        }
    }
    pub fn envoys(self) -> Vec<BughouseEnvoy> {
        match self {
            BughousePlayer::SinglePlayer(envoy) => vec![envoy],
            BughousePlayer::DoublePlayer(team) => BughouseBoard::iter().map(|board_idx| BughouseEnvoy {
                board_idx,
                force: get_bughouse_force(team, board_idx),
            }).collect(),
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
            },
            (true, false) => PlayerRelation::Opponent,
            (false, true) => PlayerRelation::Partner,
            (false, false) => PlayerRelation::Diagonal,
        }
    }
}

impl BughouseParticipant {
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
    pub fn envoys(self) -> Vec<BughouseEnvoy> {
        self.as_player().map_or(vec![], |p| p.envoys())
    }
}


#[derive(Clone, Debug)]
pub struct BughouseGame {
    starting_position: EffectiveStartingPosition,
    boards: EnumMap<BughouseBoard, Board>,
    turn_log: Vec<TurnRecordExpanded>,
    status: BughouseGameStatus,
}

impl BughouseGame {
    pub fn new(
        contest_rules: ContestRules,
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        players: &[PlayerInGame]
    ) -> Self {
        let starting_position = generate_starting_position(chess_rules.starting_position);
        Self::new_with_starting_position(contest_rules, chess_rules, bughouse_rules, starting_position, players)
    }

    pub fn new_with_starting_position(
        contest_rules: ContestRules,
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        starting_position: EffectiveStartingPosition,
        players: &[PlayerInGame],
    ) -> Self {
        let contest_rules = Rc::new(contest_rules);
        let chess_rules = Rc::new(chess_rules);
        let bughouse_rules = Rc::new(bughouse_rules);
        let player_map = make_player_map(players);
        let boards = player_map.map(|_, board_players| Board::new(
            Rc::clone(&contest_rules),
            Rc::clone(&chess_rules),
            Some(Rc::clone(&bughouse_rules)),
            board_players,
            &starting_position
        ));
        BughouseGame {
            starting_position,
            boards,
            status: BughouseGameStatus::Active,
            turn_log: Vec::new(),
        }
    }

    pub fn clone_from_start(&self) -> Self {
        Self::new_with_starting_position(
            (**self.contest_rules()).clone(),
            (**self.chess_rules()).clone(),
            (**self.bughouse_rules()).clone(),
            self.starting_position.clone(),
            &self.players()
        )
    }

    pub fn starting_position(&self) -> &EffectiveStartingPosition { &self.starting_position }
    pub fn contest_rules(&self) -> &Rc<ContestRules> { self.boards[BughouseBoard::A].contest_rules() }
    pub fn chess_rules(&self) -> &Rc<ChessRules> { self.boards[BughouseBoard::A].chess_rules() }
    pub fn bughouse_rules(&self) -> &Rc<BughouseRules> { self.boards[BughouseBoard::A].bughouse_rules().as_ref().unwrap() }
    // Improvement potential. Remove mutable access to the boards.
    pub fn board_mut(&mut self, idx: BughouseBoard) -> &mut Board { &mut self.boards[idx] }
    pub fn board(&self, idx: BughouseBoard) -> &Board { &self.boards[idx] }
    pub fn boards(&self) -> &EnumMap<BughouseBoard, Board> { &self.boards }
    pub fn reserve(&self, envoy: BughouseEnvoy) -> &Reserve {
        self.boards[envoy.board_idx].reserve(envoy.force)
    }
    pub fn turn_log(&self) -> &Vec<TurnRecordExpanded> { &self.turn_log }
    pub fn last_turn_record(&self) -> Option<&TurnRecordExpanded> { self.turn_log.last() }
    pub fn status(&self) -> BughouseGameStatus { self.status }

    pub fn players(&self) -> Vec<PlayerInGame> {
        let mut ret = vec![];
        for team in Team::iter() {
            let same_player = BughouseBoard::iter().map(|board_idx| {
                self.boards[board_idx].player_name(get_bughouse_force(team, board_idx))
            }).all_equal();
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
                        id: BughousePlayer::SinglePlayer(BughouseEnvoy{ board_idx, force }),
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
    pub fn envoy_is_active(&self, envoy: BughouseEnvoy) -> bool {
        self.status == BughouseGameStatus::Active &&
            self.boards[envoy.board_idx].active_force() == envoy.force
    }
    pub fn turn_mode_for_envoy(&self, envoy: BughouseEnvoy) -> Result<TurnMode, TurnError> {
        if self.status == BughouseGameStatus::Active {
            Ok(if self.envoy_is_active(envoy) { TurnMode::Normal } else { TurnMode::Preturn })
        } else {
            Err(TurnError::GameOver)
        }
    }

    pub fn set_status(&mut self, status: BughouseGameStatus, now: GameInstant) {
        self.status = status;
        if status != BughouseGameStatus::Active {
            for (_, board) in self.boards.iter_mut() {
                board.clock_mut().stop(now);
            }
        }
    }

    pub fn test_flag(&mut self, now: GameInstant) {
        use BughouseBoard::*;
        use BughouseGameStatus::*;
        use VictoryReason::Flag;
        assert_eq!(self.status, Active);
        self.boards[A].test_flag(now);
        self.boards[B].test_flag(now);
        let status_a = self.game_status_for_board(A);
        let status_b = self.game_status_for_board(B);
        let status = match (status_a, status_b) {
            (Victory(winner_a, Flag), Victory(winner_b, Flag)) => {
                if winner_a == winner_b {
                    Victory(winner_a, Flag)
                } else {
                    Draw(DrawReason::SimultaneousFlag)
                }
            },
            (Victory(winner, Flag), Active) => { Victory(winner, Flag) },
            (Active, Victory(winner, Flag)) => { Victory(winner, Flag) },
            (Active, Active) => { Active },
            (Victory(_, reason), _) => panic!("Unexpected victory reason in `test_flag`: {:?}", reason),
            (_, Victory(_, reason)) => panic!("Unexpected victory reason in `test_flag`: {:?}", reason),
            (Draw(_), _) | (_, Draw(_)) => panic!("Unexpected draw in `test_flag`"),
        };
        self.set_status(status, now);
    }

    // Should `test_flag` first!
    pub fn try_turn(
        &mut self, board_idx: BughouseBoard, turn_input: &TurnInput, mode: TurnMode, now: GameInstant
    )
        -> Result<Turn, TurnError>
    {
        if self.status != BughouseGameStatus::Active {
            // `Board::try_turn` will also test game status, but that's not enough: the game
            // may have ended earlier on the other board.
            return Err(TurnError::GameOver);
        }
        let board = &mut self.boards[board_idx];
        let envoy = BughouseEnvoy{ board_idx, force: board.turn_owner(mode) };
        let turn = board.parse_turn_input(turn_input, mode)?;
        // `turn_to_algebraic` must be called before `try_turn`, because algebraic form depend
        // on the current position.
        let turn_algebraic = board.turn_to_algebraic(turn, mode, AlgebraicFormat::for_log());
        let turn_facts = board.try_turn(turn, mode, now)?;
        // If `try_turn` succeeded, then the turn was valid. Thus conversion to algebraic must
        // have succeeded as well, because there exists an algebraic form for any valid turn.
        let turn_algebraic = turn_algebraic.unwrap();
        let other_board = &mut self.boards[board_idx.other()];
        match mode {
            TurnMode::Normal => { other_board.start_clock(now) }
            TurnMode::Preturn => {}
        }
        if let Some(ref capture) = turn_facts.capture {
            other_board.receive_capture(&capture);
        }
        let turn_expanded = make_turn_expanded(turn, turn_algebraic, turn_facts);
        self.turn_log.push(TurnRecordExpanded{ mode, envoy, turn_expanded, time: now });
        assert!(self.status == BughouseGameStatus::Active);
        self.set_status(self.game_status_for_board(board_idx), now);
        Ok(turn)
    }

    pub fn try_turn_by_envoy(
        &mut self, envoy: BughouseEnvoy, turn_input: &TurnInput, mode: TurnMode, now: GameInstant
    )
        -> Result<Turn, TurnError>
    {
        let expected_mode = self.turn_mode_for_envoy(envoy)?;
        if mode != expected_mode {
            return Err(TurnError::WrongTurnOrder);
        }
        self.try_turn(envoy.board_idx, turn_input, mode, now)
    }

    pub fn outcome(&self) -> String {
        use BughouseGameStatus::*;
        use VictoryReason::*;
        use DrawReason::*;
        let make_team_string = |team| {
            // Note. Not using `self.players()` because the order there is not specified.
            BughouseBoard::iter()
                .map(|board_idx| self.board(board_idx).player_name(get_bughouse_force(team, board_idx)))
                .join(" & ")
        };
        match self.status() {
            Active => "Unterminated".to_owned(),
            Victory(team, Checkmate) => format!("{} won by checkmate", make_team_string(team)),
            Victory(team, Flag) => format!("{} won by flag", make_team_string(team)),
            Victory(team, Resignation) => format!("{} won by resignation", make_team_string(team)),
            Draw(SimultaneousFlag) => "Draw by simultaneous flags".to_owned(),
            Draw(ThreefoldRepetition) => "Draw by threefold repetition".to_owned(),
        }
    }

    fn game_status_for_board(&self, board_idx: BughouseBoard) -> BughouseGameStatus {
        match self.boards[board_idx].status() {
            ChessGameStatus::Active => BughouseGameStatus::Active,
            ChessGameStatus::Victory(force, reason) =>
                BughouseGameStatus::Victory(get_bughouse_team(board_idx, force), reason),
            ChessGameStatus::Draw(reason) => BughouseGameStatus::Draw(reason),
        }
    }
}

fn make_player_map(players: &[PlayerInGame]) -> EnumMap<BughouseBoard, EnumMap<Force, String>> {
    let mut player_map: EnumMap<BughouseBoard, EnumMap<Force, Option<String>>> =
        enum_map!{ _ => enum_map!{ _ => None } };
    let mut insert_player = |board_idx, force, name: &String| {
        let player_ref = &mut player_map[board_idx][force];
        assert!(player_ref.is_none());
        *player_ref = Some(name.clone());
    };
    for p in players {
        match p.id {
            BughousePlayer::SinglePlayer(envoy) => {
                insert_player(envoy.board_idx, envoy.force, &p.name);
            },
            BughousePlayer::DoublePlayer(team) => {
                for board_idx in BughouseBoard::iter() {
                    insert_player(board_idx, get_bughouse_force(team, board_idx), &p.name);
                }
            },
        }
    }
    player_map.map(|_, board_players| {
        board_players.map(|_, p| { p.unwrap() })
    })
}

fn make_turn_expanded(turn: Turn, algebraic: String, facts: TurnFacts) -> TurnExpanded {
    let mut relocation = None;
    let mut relocation_extra = None;
    let mut drop = None;
    match turn {
        Turn::Move(mv) => {
            relocation = Some((mv.from, mv.to));
        },
        Turn::Drop(dr) => {
            drop = Some(dr.to);
        },
        Turn::Castle(_) => {
            let castling_relocations = facts.castling_relocations.unwrap();
            relocation = Some(castling_relocations.king);
            relocation_extra = Some(castling_relocations.rook);
        },
    }
    TurnExpanded {
        turn,
        algebraic_for_log: algebraic,
        relocation,
        relocation_extra,
        drop,
        capture: facts.capture,
    }
}
