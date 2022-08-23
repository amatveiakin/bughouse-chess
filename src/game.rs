#![allow(unused_parens)]

use std::rc::Rc;

use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;
use rand::prelude::*;
use serde::{Serialize, Deserialize};
use strum::EnumIter;

use crate::NUM_ROWS;
use crate::board::{Board, Turn, TurnInput, TurnMode, TurnError, ChessGameStatus, VictoryReason, DrawReason};
use crate::clock::GameInstant;
use crate::coord::{Row, Col, Coord};
use crate::force::Force;
use crate::grid::Grid;
use crate::piece::{PieceKind, PieceOrigin, PieceOnBoard};
use crate::player::{PlayerInGame, Team};
use crate::rules::{StartingPosition, ChessRules, BughouseRules};


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnRecord {
    pub player_id: BughousePlayerId,
    pub turn_algebraic: String,
    pub time: GameInstant,
}

fn new_white(kind: PieceKind) -> PieceOnBoard {
    PieceOnBoard::new(kind, PieceOrigin::Innate, Force::White)
}

fn setup_white_pawns_on_2nd_row(grid: &mut Grid) {
    for col in Col::all() {
        grid[Coord::new(Row::_2, col)] = Some(new_white(PieceKind::Pawn));
    }
}

fn setup_black_pieces_mirrorlike(grid: &mut Grid) {
    for coord in Coord::all() {
        if let Some(piece) = grid[coord] {
            if piece.force == Force::White {
                let mirror_row = Row::from_zero_based(NUM_ROWS - coord.row.to_zero_based() - 1);
                let mirror_coord = Coord::new(mirror_row, coord.col);
                assert!(grid[mirror_coord].is_none(), "{:?}", grid);
                grid[mirror_coord] = Some(PieceOnBoard {
                    force: Force::Black,
                    ..piece
                });
            }
        }
    }
}

fn generate_starting_grid(starting_position: StartingPosition) -> Grid {
    use PieceKind::*;
    match starting_position {
        StartingPosition::Classic => {
            let mut grid = Grid::new();
            grid[Coord::A1] = Some(new_white(Rook));
            grid[Coord::B1] = Some(new_white(Knight));
            grid[Coord::C1] = Some(new_white(Bishop));
            grid[Coord::D1] = Some(new_white(Queen));
            grid[Coord::E1] = Some(new_white(King));
            grid[Coord::F1] = Some(new_white(Bishop));
            grid[Coord::G1] = Some(new_white(Knight));
            grid[Coord::H1] = Some(new_white(Rook));
            setup_white_pawns_on_2nd_row(&mut grid);
            setup_black_pieces_mirrorlike(&mut grid);
            grid
        },
        StartingPosition::FischerRandom => {
            let mut rng = rand::thread_rng();
            let mut grid = Grid::new();
            let row = Row::_1;
            grid[Coord::new(row, Col::from_zero_based(rng.gen_range(0..4) * 2))] = Some(new_white(Bishop));
            grid[Coord::new(row, Col::from_zero_based(rng.gen_range(0..4) * 2 + 1))] = Some(new_white(Bishop));
            let mut cols = Col::all().filter(|col| grid[Coord::new(row, *col)].is_none()).collect_vec();
            cols.shuffle(&mut rng);
            let (king_and_rook_cols, queen_and_knight_cols) = cols.split_at(3);
            let (&left_rook_col, &king_col, &right_rook_col) =
                king_and_rook_cols.iter().sorted().collect_tuple().unwrap();
            let (&queen_col, &knight_col_1, &knight_col_2) =
                queen_and_knight_cols.iter().collect_tuple().unwrap();
            grid[Coord::new(row, left_rook_col)] = Some(new_white(Rook));
            grid[Coord::new(row, king_col)] = Some(new_white(King));
            grid[Coord::new(row, right_rook_col)] = Some(new_white(Rook));
            grid[Coord::new(row, queen_col)] = Some(new_white(Queen));
            grid[Coord::new(row, knight_col_1)] = Some(new_white(Knight));
            grid[Coord::new(row, knight_col_2)] = Some(new_white(Knight));
            setup_white_pawns_on_2nd_row(&mut grid);
            setup_black_pieces_mirrorlike(&mut grid);
            grid
        },
    }
}


#[derive(Clone, Debug)]
pub struct ChessGame {
    board: Board,
}

impl ChessGame {
    pub fn new(rules: ChessRules, players: EnumMap<Force, Rc<PlayerInGame>>) -> Self {
        let starting_grid = generate_starting_grid(rules.starting_position);
        Self::new_with_grid(rules, starting_grid, players)
    }

    pub fn new_with_grid(
        rules: ChessRules,
        starting_grid: Grid,
        players: EnumMap<Force, Rc<PlayerInGame>>
    ) -> Self {
        ChessGame {
            board: Board::new(
                Rc::new(rules),
                None,
                players,
                starting_grid,
            ),
        }
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
}

// TODO: Unify board flipping for tui and web clients
#[derive(Clone, Copy, Debug)]
pub struct BughouseGameView {
    pub flip_boards: bool,
    pub flip_forces: bool,
}

impl BughouseGameView {
    pub fn for_player(player_id: BughousePlayerId) -> Self {
        use BughouseBoard::*;
        use Force::*;
        BughouseGameView {
            flip_boards: match player_id.board_idx { A => false, B => true },
            flip_forces: match player_id.force { White => false, Black => true },
        }
    }
}

#[derive(Clone, Debug)]
pub struct BughouseGame {
    boards: EnumMap<BughouseBoard, Board>,
    turn_log: Vec<TurnRecord>,
    status: BughouseGameStatus,
}

impl BughouseGame {
    pub fn new(
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        players: EnumMap<BughouseBoard, EnumMap<Force, Rc<PlayerInGame>>>
    ) -> Self {
        let starting_grid = generate_starting_grid(chess_rules.starting_position);
        Self::new_with_grid(chess_rules, bughouse_rules, starting_grid, players)
    }

    pub fn new_with_grid(
        chess_rules: ChessRules,
        bughouse_rules: BughouseRules,
        starting_grid: Grid,
        players: EnumMap<BughouseBoard, EnumMap<Force, Rc<PlayerInGame>>>
    ) -> Self {
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
            boards,
            status: BughouseGameStatus::Active,
            turn_log: Vec::new(),
        }
    }

    pub fn make_player_map(players: impl Iterator<Item = (Rc<PlayerInGame>, BughouseBoard)>)
        -> EnumMap<BughouseBoard, EnumMap<Force, Rc<PlayerInGame>>>
    {
        let mut player_map: EnumMap<BughouseBoard, EnumMap<Force, Option<Rc<PlayerInGame>>>> =
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

    pub fn chess_rules(&self) -> &Rc<ChessRules> { self.boards[BughouseBoard::A].chess_rules() }
    pub fn bughouse_rules(&self) -> &Rc<BughouseRules> { self.boards[BughouseBoard::A].bughouse_rules().as_ref().unwrap() }
    // Improvement potential. Remove mutable access to the boards.
    pub fn board_mut(&mut self, idx: BughouseBoard) -> &mut Board { &mut self.boards[idx] }
    pub fn board(&self, idx: BughouseBoard) -> &Board { &self.boards[idx] }
    pub fn boards(&self) -> &EnumMap<BughouseBoard, Board> { &self.boards }
    pub fn players(&self) -> Vec<Rc<PlayerInGame>> {
        self.boards.values().flat_map(|(board)| board.players().values().cloned()).collect()
    }
    pub fn turn_log(&self) -> &Vec<TurnRecord> { &self.turn_log }
    pub fn last_turn_record(&self) -> Option<&TurnRecord> { self.turn_log.last() }
    pub fn status(&self) -> BughouseGameStatus { self.status }

    pub fn find_player(&self, player_name: &str) -> Option<BughousePlayerId> {
        for (board_idx, board) in self.boards.iter() {
            for (force, player) in board.players() {
                if player.name == player_name {
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
            // `Board::try_turn` will also test status, but that's not enough: the game
            // may have ended earlier on the other board.
            return Err(TurnError::GameOver);
        }
        let board = &mut self.boards[board_idx];
        let player_id = BughousePlayerId{ board_idx, force: board.active_force() };
        let turn = board.parse_turn_input(turn_input, mode)?;
        let turn_algebraic = board.turn_to_algebraic(turn)?;
        let capture_or = board.try_turn(turn, mode, now)?;
        let other_board = &mut self.boards[board_idx.other()];
        other_board.start_clock(now);
        if let Some(capture) = capture_or {
            other_board.receive_capture(&capture);
        }
        self.turn_log.push(TurnRecord{ player_id, turn_algebraic, time: now });
        assert!(self.status == BughouseGameStatus::Active);
        self.set_status(self.game_status_for_board(board_idx), now);
        Ok(turn)
    }

    pub fn try_turn_by_player(
        &mut self, player_id: BughousePlayerId, turn_input: &TurnInput, mode: TurnMode, now: GameInstant
    )
        -> Result<Turn, TurnError>
    {
        if mode != self.turn_mode_for_player(player_id)? {
            return Err(TurnError::WrongTurnOrder);
        }
        self.try_turn(player_id.board_idx, turn_input, mode, now)
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
