#![allow(unused_parens)]

use std::rc::Rc;

use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use serde::{Serialize, Deserialize};
use strum::{EnumIter, IntoEnumIterator};

use crate::board::{Board, Reserve, Turn, TurnInput, TurnExpanded, TurnFacts, TurnMode, TurnError, ChessGameStatus, VictoryReason, DrawReason};
use crate::clock::GameInstant;
use crate::force::Force;
use crate::piece::piece_to_pictogram;
use crate::player::Team;
use crate::rules::{ContestRules, ChessRules, BughouseRules};
use crate::starter::{EffectiveStartingPosition, generate_starting_position};


// Stripped version of `TurnRecordExpanded`. For sending turns across network.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnRecord {
    pub player_id: BughousePlayerId,
    pub turn_algebraic: String,
    pub time: GameInstant,
}

#[derive(Clone, Debug)]
pub struct TurnRecordExpanded {
    pub mode: TurnMode,
    pub player_id: BughousePlayerId,
    pub turn_expanded: TurnExpanded,
    pub time: GameInstant,
}

impl TurnRecordExpanded {
    pub fn trim_for_sending(&self) -> TurnRecord {
        // This is used only to send confirmed turns from server to clients, so preturns
        // should never occur here.
        assert_eq!(self.mode, TurnMode::Normal);
        TurnRecord {
            player_id: self.player_id,
            turn_algebraic: self.turn_expanded.algebraic.clone(),
            time: self.time,
        }
    }

    pub fn to_log_entry(&self) -> String {
        let algebraic = &self.turn_expanded.algebraic;
        let s = if let Some(capture) = self.turn_expanded.capture {
            let capture = piece_to_pictogram(capture.piece_kind, capture.force);
            format!("{algebraic} ·{capture}")
        } else {
            format!("{algebraic}")
        };
        match self.mode {
            TurnMode::Normal => s,
            TurnMode::Preturn => format!("({s})"),
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

// TODO: Factor out this and other defines for bughouse.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct BughousePlayerId {
    pub board_idx: BughouseBoard,
    pub force: Force,
}

// Player in an active game.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerInGame {
    pub name: String,
    pub id: BughousePlayerId,
}

// Describes the player with whose eyes the game is viewed. This is not really an ID in that
// it's not unique: many observers can view the game through the same lens.
// Improvement potential. Rename BughouseObserserId and BughouseParticipantId to reflect that.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BughouseObserserId {
    pub board_idx: BughouseBoard,
    pub force: Force,
}

// Describes a participant who is either playing or observing the game. This is not really an
// ID in that it's not unique: many observers can view the game through the same lens.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BughouseParticipantId {
    Player(BughousePlayerId),
    Observer(BughouseObserserId)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
pub enum PlayerRelation {
    Myself,
    Opponent,
    Partner,
    Diagonal,
    Other,  // either me or the other party is not a player
}

impl BughousePlayerId {
    pub fn team(self) -> Team {
        get_bughouse_team(self.board_idx, self.force)
    }
    pub fn opponent(self) -> Self {
        BughousePlayerId {
            board_idx: self.board_idx,
            force: self.force.opponent(),
        }
    }

    pub fn relation_to(self, other_player: BughousePlayerId) -> PlayerRelation {
        let same_board = self.board_idx == other_player.board_idx;
        let same_team = self.team() == other_player.team();
        match (same_board, same_team) {
            (true, true) => PlayerRelation::Myself,
            (true, false) => PlayerRelation::Opponent,
            (false, true) => PlayerRelation::Partner,
            (false, false) => PlayerRelation::Diagonal,
        }
    }
}

impl BughouseParticipantId {
    // To be used for rendering purposes only. If actions are to be taken on a board, use
    // `let BughouseParticipantId::Player(...) = ...` to check that this is an actual player.
    pub fn visual_board_idx(self) -> BughouseBoard {
        match self {
            Self::Player(id) => id.board_idx,
            Self::Observer(id) => id.board_idx,
        }
    }
    // To be used for rendering purposes only. If actions are to be taken on behalf of a force, use
    // `let BughouseParticipantId::Player(...) = ...` to check that this is an actual player.
    pub fn visual_force(self) -> Force {
        match self {
            Self::Player(id) => id.force,
            Self::Observer(id) => id.force,
        }
    }
    // To be used for rendering purposes only. If actions are to be taken on behalf of a team, use
    // `let BughouseParticipantId::Player(...) = ...` to check that this is an actual player.
    pub fn visual_team(self) -> Team {
        get_bughouse_team(self.visual_board_idx(), self.visual_force())
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

    pub fn starting_position(&self) -> &EffectiveStartingPosition { &self.starting_position }
    pub fn contest_rules(&self) -> &Rc<ContestRules> { self.boards[BughouseBoard::A].contest_rules() }
    pub fn chess_rules(&self) -> &Rc<ChessRules> { self.boards[BughouseBoard::A].chess_rules() }
    pub fn bughouse_rules(&self) -> &Rc<BughouseRules> { self.boards[BughouseBoard::A].bughouse_rules().as_ref().unwrap() }
    // Improvement potential. Remove mutable access to the boards.
    pub fn board_mut(&mut self, idx: BughouseBoard) -> &mut Board { &mut self.boards[idx] }
    pub fn board(&self, idx: BughouseBoard) -> &Board { &self.boards[idx] }
    pub fn boards(&self) -> &EnumMap<BughouseBoard, Board> { &self.boards }
    pub fn reserve(&self, player_id: BughousePlayerId) -> &Reserve {
        self.boards[player_id.board_idx].reserve(player_id.force)
    }
    pub fn turn_log(&self) -> &Vec<TurnRecordExpanded> { &self.turn_log }
    pub fn last_turn_record(&self) -> Option<&TurnRecordExpanded> { self.turn_log.last() }
    pub fn status(&self) -> BughouseGameStatus { self.status }

    pub fn players(&self) -> Vec<PlayerInGame> {
        self.boards.iter().flat_map(|(board_idx, board)|
            board.player_names().iter().map(move |(force, name)| PlayerInGame {
                name: name.to_owned(),
                id: BughousePlayerId{ board_idx, force }
            })
        ).collect()
    }
    pub fn find_player(&self, player_name: &str) -> Option<BughousePlayerId> {
        for (board_idx, board) in self.boards.iter() {
            for (force, name) in board.player_names() {
                if name == player_name {
                    return Some(BughousePlayerId{ board_idx, force });
                }
            }
        }
        None
    }
    pub fn player_is_active(&self, player_id: BughousePlayerId) -> bool {
        self.status == BughouseGameStatus::Active &&
            self.boards[player_id.board_idx].active_force() == player_id.force
    }
    pub fn turn_mode_for_player(&self, player_id: BughousePlayerId) -> Result<TurnMode, TurnError> {
        if self.status == BughouseGameStatus::Active {
            Ok(if self.player_is_active(player_id) { TurnMode::Normal } else { TurnMode::Preturn })
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
        let player_id = BughousePlayerId{ board_idx, force: board.turn_owner(mode) };
        let turn = board.parse_turn_input(turn_input, mode)?;
        // `turn_to_algebraic` must be called before `try_turn`, because algebraic form depend
        // on the current position.
        let turn_algebraic = board.turn_to_algebraic(turn, mode);
        let turn_facts = board.try_turn(turn, mode, now)?;
        // If `try_turn` succeeded, then the turn was valid. Thus conversion to algebraic must
        // have succeeded as well, because there exists an algebraic form for any valid turn.
        let turn_algebraic = turn_algebraic.unwrap();
        let other_board = &mut self.boards[board_idx.other()];
        match mode {
            TurnMode::Normal => { other_board.start_clock(now) }
            TurnMode::Preturn => {}
        }
        if let Some(capture) = turn_facts.capture {
            other_board.receive_capture(&capture);
        }
        let turn_expanded = make_turn_expanded(turn, turn_algebraic.clone(), turn_facts);
        self.turn_log.push(TurnRecordExpanded{ mode, player_id, turn_expanded, time: now });
        assert!(self.status == BughouseGameStatus::Active);
        self.set_status(self.game_status_for_board(board_idx), now);
        Ok(turn)
    }

    pub fn try_turn_by_player(
        &mut self, player_id: BughousePlayerId, turn_input: &TurnInput, mode: TurnMode, now: GameInstant
    )
        -> Result<Turn, TurnError>
    {
        let expected_mode = self.turn_mode_for_player(player_id)?;
        if mode != expected_mode {
            return Err(TurnError::WrongTurnOrder);
        }
        self.try_turn(player_id.board_idx, turn_input, mode, now)
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
    for p in players {
        let player_ref = &mut player_map[p.id.board_idx][p.id.force];
        assert!(player_ref.is_none());
        *player_ref = Some(p.name.clone());
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
        algebraic,
        relocation,
        relocation_extra,
        drop,
        capture: facts.capture,
    }
}
