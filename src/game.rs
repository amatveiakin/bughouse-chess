#![allow(unused_parens)]

use std::rc::Rc;

use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use lazy_static::lazy_static;
use rand::prelude::*;
use regex::Regex;
use serde::{Serialize, Deserialize};

use crate::board::{Board, Turn, TurnError, ChessGameStatus, VictoryReason, turn_from_algebraic};
use crate::clock::GameInstant;
use crate::coord::{Row, Col, Coord};
use crate::force::Force;
use crate::grid::Grid;
use crate::piece::{PieceKind, PieceOrigin, PieceOnBoard, CastleDirection};
use crate::player::{Player, Team};
use crate::rules::{StartingPosition, ChessRules, BughouseRules};


fn generate_starting_grid(starting_position: StartingPosition) -> Grid {
    use CastleDirection::*;
    use PieceKind::*;
    let new_white = |kind| {
        assert_ne!(kind, Rook);
        PieceOnBoard::new(kind, PieceOrigin::Innate, None, Force::White)
    };
    let new_white_rook = |castling| {
        PieceOnBoard::new(Rook, PieceOrigin::Innate, Some(castling), Force::White)
    };
    let mut grid = Grid::new();

    for col in Col::all() {
        grid[Coord::new(Row::_2, col)] = Some(new_white(Pawn));
    }
    match starting_position {
        StartingPosition::Classic => {
            grid[Coord::A1] = Some(new_white_rook(ASide));
            grid[Coord::B1] = Some(new_white(Knight));
            grid[Coord::C1] = Some(new_white(Bishop));
            grid[Coord::D1] = Some(new_white(Queen));
            grid[Coord::E1] = Some(new_white(King));
            grid[Coord::F1] = Some(new_white(Bishop));
            grid[Coord::G1] = Some(new_white(Knight));
            grid[Coord::H1] = Some(new_white_rook(HSide));
        },
        StartingPosition::FischerRandom => {
            let mut rng = rand::thread_rng();
            let row = Row::_1;
            grid[Coord::new(row, Col::from_zero_based(rng.gen_range(0..4) * 2))] = Some(new_white(Bishop));
            grid[Coord::new(row, Col::from_zero_based(rng.gen_range(0..4) * 2 + 1))] = Some(new_white(Bishop));
            let mut cols = Col::all().filter(|col| grid[Coord::new(row, *col)].is_none()).collect_vec();
            cols.shuffle(&mut rng);
            let (king_and_rook_cols, queen_and_knight_cols) = cols.split_at(3);
            let [&left_rook_col, &king_col, &right_rook_col] =
                <[&Col; 3]>::try_from(king_and_rook_cols.into_iter().sorted().collect_vec()).unwrap();
            let [queen_col, knight_col_1, knight_col_2] =
                <[Col; 3]>::try_from(queen_and_knight_cols).unwrap();
            grid[Coord::new(row, left_rook_col)] = Some(new_white_rook(ASide));
            grid[Coord::new(row, king_col)] = Some(new_white(King));
            grid[Coord::new(row, right_rook_col)] = Some(new_white_rook(HSide));
            grid[Coord::new(row, queen_col)] = Some(new_white(Queen));
            grid[Coord::new(row, knight_col_1)] = Some(new_white(Knight));
            grid[Coord::new(row, knight_col_2)] = Some(new_white(Knight));
        },
    }

    for col in Col::all() {
        grid[Coord::new(Row::_7, col)] = grid[Coord::new(Row::_2, col)].map(|mut piece| {
            piece.force = Force::Black;
            piece
        });
        grid[Coord::new(Row::_8, col)] = grid[Coord::new(Row::_1, col)].map(|mut piece| {
            piece.force = Force::Black;
            piece
        });
    }
    grid
}


#[derive(Clone, Debug)]
pub struct ChessGame {
    board: Board,
}

impl ChessGame {
    pub fn new(rules: ChessRules, players: EnumMap<Force, Rc<Player>>) -> ChessGame {
        let starting_position = rules.starting_position;
        ChessGame {
            board: Board::new(
                Rc::new(rules),
                None,
                players,
                generate_starting_grid(starting_position)
            ),
        }
    }

    pub fn board(&self) -> &Board { &self.board }
    pub fn status(&self) -> ChessGameStatus { self.board.status() }

    pub fn test_flag(&mut self, now: GameInstant) {
        self.board.test_flag(now);
    }

    // Should `test_flag` first!
    pub fn try_turn(&mut self, turn: Turn, now: GameInstant) -> Result<(), TurnError> {
        self.board.try_turn(turn, now)?;
        Ok(())
    }
    pub fn try_turn_from_algebraic(&mut self, notation: &str, now: GameInstant) -> Result<(), TurnError> {
        let active_force = self.board.active_force();
        let turn = turn_from_algebraic(self.board.grid_mut(), active_force, notation)?;
        self.try_turn(turn, now)
    }
    pub fn try_replay_log(&mut self, log: &str) -> Result<(), TurnError> {
        lazy_static! {
            static ref TURN_NUMBER_RE: Regex = Regex::new(r"^(?:[0-9]+\.)?(.*)$").unwrap();
        }
        // TODO: What should happen to time when replaying log?
        let now = GameInstant::game_start();
        for turn_notation in log.split_whitespace() {
            let turn_notation = TURN_NUMBER_RE.captures(turn_notation).unwrap().get(1).unwrap().as_str();
            self.try_turn_from_algebraic(turn_notation, now)?
        }
        Ok(())
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum, Serialize, Deserialize)]
pub enum BughouseBoard {
    A,
    B,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BughouseGameStatus {
    Active,
    Victory(Team, VictoryReason),
    Draw,
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

#[derive(Clone, Debug)]
pub struct BughouseGame {
    boards: EnumMap<BughouseBoard, Board>,
    status: BughouseGameStatus,
}

impl BughouseGame {
    pub fn new(
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        players: EnumMap<BughouseBoard, EnumMap<Force, Rc<Player>>>
    ) -> BughouseGame {
        let starting_grid = generate_starting_grid(chess_rules.starting_position);
        Self::new_with_grid(chess_rules, bughouse_rules, starting_grid, players)
    }

    pub fn new_with_grid(
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        starting_grid: Grid,
        players: EnumMap<BughouseBoard, EnumMap<Force, Rc<Player>>>
    ) -> BughouseGame {
        let chess_rules = Rc::new(chess_rules);
        let bughouse_rules = Rc::new(bughouse_rules);
        let boards = enum_map!{
            BughouseBoard::A => Board::new(
                Rc::clone(&chess_rules),
                Some(Rc::clone(&bughouse_rules)),
                players[BughouseBoard::A].clone(),
                starting_grid.clone()
            ),
            BughouseBoard::B => Board::new(
                Rc::clone(&chess_rules),
                Some(Rc::clone(&bughouse_rules)),
                players[BughouseBoard::B].clone(),
                starting_grid.clone()
            ),
        };
        BughouseGame {
            boards: boards,
            status: BughouseGameStatus::Active,
        }
    }

    pub fn make_player_map(players: impl Iterator<Item = (Rc<Player>, BughouseBoard)>)
        -> EnumMap<BughouseBoard, EnumMap<Force, Rc<Player>>>
    {
        let mut player_map: EnumMap<BughouseBoard, EnumMap<Force, Option<Rc<Player>>>> =
            enum_map!{ _ => enum_map!{ _ => None } };
        for (p, board_idx) in players {
            let player_ref = &mut player_map[board_idx][get_bughouse_force(p.team, board_idx)];
            assert!(player_ref.is_none());
            *player_ref = Some(p);
        }
        player_map.map(|_, board_players| {
            board_players.map(|_, p| { p.unwrap() })
        })
    }

    pub fn board(&self, idx: BughouseBoard) -> &Board { &self.boards[idx] }
    pub fn status(&self) -> BughouseGameStatus { self.status }

    pub fn find_player(&self, player_name: &str) -> Option<(BughouseBoard, Force)> {
        for (board_idx, board) in self.boards.iter() {
            for (force, player) in board.players() {
                if player.name == player_name {
                    return Some((board_idx, force));
                }
            }
        }
        None
    }
    pub fn player_board(&self, player_name: &str) -> Option<&Board> {
        self.find_player(player_name).map(|(board_idx, _)| &self.boards[board_idx])
    }
    pub fn player_is_active(&self, player_name: &str) -> Option<bool> {
        self.find_player(player_name).map(|(board_idx, force)| {
            self.boards[board_idx].active_force() == force
        })
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
        assert!(self.status == Active);
        self.boards[A].test_flag(now);
        self.boards[B].test_flag(now);
        let status_a = self.game_status_for_board(A);
        let status_b = self.game_status_for_board(B);
        let status = match (status_a, status_b) {
            (Victory(victory_a, Flag), Victory(victory_b, Flag)) => {
                if victory_a == victory_b { Victory(victory_a, Flag) } else { Draw }
            },
            (Victory(victory, Flag), Active) => { Victory(victory, Flag) },
            (Active, Victory(victory, Flag)) => { Victory(victory, Flag) },
            (Active, Active) => { Active },
            (Victory(_, reason), _) => panic!("Unexpected victory reason in `test_flag`: {:?}", reason),
            (_, Victory(_, reason)) => panic!("Unexpected victory reason in `test_flag`: {:?}", reason),
            (Draw, _) | (_, Draw) => panic!("Unexpected draw in `test_flag`"),
        };
        self.set_status(status, now);
    }

    // Should `test_flag` first!
    pub fn try_turn(&mut self, board_idx: BughouseBoard, turn: Turn, now: GameInstant)
        -> Result<(), TurnError>
    {
        let capture_or = self.boards[board_idx].try_turn(turn, now)?;
        self.boards[board_idx.other()].start_clock(now);
        if let Some(capture) = capture_or {
            self.boards[board_idx.other()].receive_capture(&capture)
        }
        assert!(self.status == BughouseGameStatus::Active);
        self.set_status(self.game_status_for_board(board_idx), now);
        Ok(())
    }
    pub fn try_turn_from_algebraic(&mut self, board_idx: BughouseBoard, notation: &str, now: GameInstant)
        -> Result<(), TurnError>
    {
        let active_force = self.boards[board_idx].active_force();
        let turn = turn_from_algebraic(self.boards[board_idx].grid_mut(), active_force, notation)?;
        self.try_turn(board_idx, turn, now)
    }
    pub fn try_turn_by_player_from_algebraic(&mut self, player_name: &str, notation: &str, now: GameInstant)
        -> Result<(), TurnError>
    {
        let (board_idx, force) = self.find_player(player_name).unwrap_or_else(
            || panic!("Player not found: {}", player_name));
        if force != self.board(board_idx).active_force() {
            return Err(TurnError::WrongTurnOrder);
        }
        self.try_turn_from_algebraic(board_idx, notation, now)
    }

    fn game_status_for_board(&self, board_idx: BughouseBoard) -> BughouseGameStatus {
        match self.boards[board_idx].status() {
            ChessGameStatus::Active => BughouseGameStatus::Active,
            ChessGameStatus::Victory(force, reason) =>
                BughouseGameStatus::Victory(get_bughouse_team(board_idx, force), reason),
            ChessGameStatus::Draw => BughouseGameStatus::Draw,
        }
    }
}


// TODO: Flip boards and sides to make current player appear in the bottom left corner
// pub struct BughouseGameView {
//     flip_boards: bool,
//     blip_forces: bool,
// }
// pub fn view_for_player(game: &BughouseGame, player: &Player) -> BughouseGameView {
// }
