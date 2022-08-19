// Improvement potential. Chess draws: dead position, stalemate, fifty-move rule.

#![allow(unused_parens)]

use std::collections::HashMap;
use std::rc::Rc;

use enum_map::{EnumMap, enum_map};
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Serialize, Deserialize};
use strum::IntoEnumIterator;

use crate::coord::{SubjectiveRow, Row, Col, Coord};
use crate::clock::{GameInstant, Clock};
use crate::force::Force;
use crate::grid::{Grid, GridForRepetitionDraw};
use crate::piece::{PieceKind, PieceOrigin, PieceOnBoard, PieceForRepetitionDraw, CastleDirection};
use crate::player::PlayerInGame;
use crate::rules::{DropAggression, ChessRules, BughouseRules};
use crate::util::{sort_two, as_single_char};


fn iter_minmax<T: PartialOrd + Copy, I: Iterator<Item = T>>(iter: I) -> Option<(T, T)> {
    match iter.minmax() {
        itertools::MinMaxResult::NoElements => None,
        itertools::MinMaxResult::OneElement(v) => Some((v, v)),
        itertools::MinMaxResult::MinMax(min, max) => Some((min, max)),
    }
}

fn direction_forward(force: Force) -> i8 {
    match force {
        Force::White => 1,
        Force::Black => -1,
    }
}

fn col_range_inclusive((col_min, col_max): (Col, Col)) -> impl Iterator<Item = Col> {
    assert!(col_min <= col_max);
    (col_min.to_zero_based() ..= col_max.to_zero_based()).map(|idx| Col::from_zero_based(idx))
}

fn find_king(grid: &Grid, force: Force) -> Option<Coord> {
    for pos in Coord::all() {
        if let Some(piece) = grid[pos] {
            if piece.kind == PieceKind::King && piece.force == force {
                return Some(pos);
            }
        }
    }
    None
}

fn should_promote(force: Force, piece_kind: PieceKind, to: Coord) -> bool {
    let last_row = SubjectiveRow::from_one_based(8).to_row(force);
    piece_kind == PieceKind::Pawn && to.row == last_row
}

fn can_promote_to(piece_kind: PieceKind) -> bool {
    use PieceKind::*;
    match piece_kind {
        Pawn | King => false,
        Knight | Bishop | Rook | Queen => true,
    }
}

fn get_capture(grid: &Grid, from: Coord, to: Coord, en_passant_target: Option<Coord>) -> Option<Coord> {
    let piece = grid[from].unwrap();
    if let Some(target_piece) = grid[to] {
        if target_piece.force != piece.force {
            return Some(to);
        }
    } else if let Some(en_passant_target) = en_passant_target {
        if piece.kind == PieceKind::Pawn && to == en_passant_target {
            let row = en_passant_target.row + direction_forward(piece.force.opponent());
            return Some(Coord::new(row, en_passant_target.col));
        }
    }
    None
}

fn get_en_passant_target(grid: &Grid, turn: Turn) -> Option<Coord> {
    if let Turn::Move(mv) = turn {
        let piece_kind = grid[mv.to].unwrap().kind;
        if piece_kind == PieceKind::Pawn &&
            mv.to.col == mv.from.col &&
            (mv.to.row - mv.from.row).abs() == 2
        {
            let row_idx = (mv.to.row.to_zero_based() + mv.from.row.to_zero_based()) / 2;
            let row = Row::from_zero_based(row_idx);
            return Some(Coord::new(row, mv.to.col));
        }
    }
    None
}

// Generates move candidates to test whether a player can escape a mate via normal
// chess (not bughouse) moves.
// Simplifications:
//   - Does not generate castles since castling cannot be done while checked.
//   - Pawnes are not promoted.
//   - Drops are not generated (this is done separately in `is_bughouse_mate_to`).
fn generate_moves_for_mate_test(grid: &mut Grid, from: Coord, en_passant_target: Option<Coord>)
    -> Vec<TurnMove>
{
    // Improvement potential: Don't iterate over all squares.
    let mut moves = Vec::new();
    for to in Coord::all() {
        let capture_or = get_capture(grid, from, to, en_passant_target);
        if reachability(grid, from, to, capture_or.is_some()).ok() {
            moves.push(TurnMove{ from, to, promote_to: None });
        }
    }
    moves
}

fn king_force(grid: &Grid, king_pos: Coord) -> Force {
    let piece = grid[king_pos].unwrap();
    assert_eq!(piece.kind, PieceKind::King);
    piece.force
}

// Grid is guaratneed to be returned intact.
fn is_chess_mate_to(grid: &mut Grid, king_pos: Coord, en_passant_target: Option<Coord>) -> bool {
    if !is_check_to(grid, king_pos) {
        return false;
    }
    let force = king_force(grid, king_pos);
    for pos in Coord::all() {
        if let Some(piece) = grid[pos] {
            if piece.force == force {
                for mv in generate_moves_for_mate_test(grid, pos, en_passant_target) {
                    let capture_or = get_capture(grid, mv.from, mv.to, en_passant_target);
                    // Zero out capture separately because of en passant.
                    let mut grid = grid.maybe_scoped_set(capture_or.map(|pos| (pos, None)));
                    let mut grid = grid.scoped_set(mv.from, None);
                    let mut grid = grid.scoped_set(mv.to, Some(piece));
                    let new_king_pos = if piece.kind == PieceKind::King { mv.to } else { king_pos };
                    if !is_check_to(&mut grid, new_king_pos) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// Grid is guaratneed to be returned intact.
fn is_bughouse_mate_to(grid: &mut Grid, king_pos: Coord, en_passant_target: Option<Coord>) -> bool {
    let force = king_force(grid, king_pos);
    if !is_chess_mate_to(grid, king_pos, en_passant_target) {
        return false;
    }
    for pos in Coord::all() {
        if grid[pos].is_none() {
            let mut grid = grid.scoped_set(pos, Some(PieceOnBoard::new(
                PieceKind::Queen, PieceOrigin::Dropped, force
            )));
            if !is_check_to(&mut grid, king_pos) {
                return false;
            }
        }
    }
    true
}

fn is_check_to(grid: &Grid, king_pos: Coord) -> bool {
    let force = king_force(grid, king_pos);
    for from in Coord::all() {
        if let Some(piece) = grid[from] {
            if piece.force != force && reachability(grid, from, king_pos, true).ok() {
                return true;
            }
        }
    }
    false
}

fn reachability(grid: &Grid, from: Coord, to: Coord, capturing: bool) -> Reachability {
    use ProtoReachability::*;
    match proto_reachability(grid, from, to) {
        Ok => Reachability::Ok,
        OkIfCapturing => if capturing { Reachability::Ok } else { Reachability::Blocked },
        OkIfNonCapturing => if !capturing { Reachability::Ok } else { Reachability::Blocked },
        Blocked => Reachability::Blocked,
        Impossible => Reachability::Impossible,
    }
}

fn is_reachable_for_premove(grid: &Grid, from: Coord, to: Coord) -> bool {
    use ProtoReachability::*;
    match proto_reachability(grid, from, to) {
        Ok | OkIfCapturing | OkIfNonCapturing | Blocked => true,
        Impossible => false,
    }
}

// Tests that the piece can move in such a way and that the path is free.
// Does not support castling.
fn proto_reachability(grid: &Grid, from: Coord, to: Coord) -> ProtoReachability {
    use ProtoReachability::*;
    let proto = proto_reachability_modulo_destination_square(grid, from, to);
    match proto {
        Blocked | Impossible => proto,
        Ok | OkIfCapturing | OkIfNonCapturing => {
            if let Some(piece) = grid[to] {
                if piece.force == grid[from].unwrap().force {
                    return Blocked;
                }
            }
            proto
        }
    }
}

fn proto_reachability_modulo_destination_square(grid: &Grid, from: Coord, to: Coord)
    -> ProtoReachability
{
    use ProtoReachability::*;
    if to == from {
        return Impossible;
    }
    let force;
    let piece_kind;
    match grid[from] {
        Some(piece) => {
            force = piece.force;
            piece_kind = piece.kind;
        },
        None => {
            return Impossible;
        },
    }

    let (d_row, d_col) = to - from;
    let is_straight_move = d_row == 0 || d_col == 0;
    let is_diagonal_move = d_row.abs() == d_col.abs();
    // Tests that squares between `from` (exclusive) and `to` (exclusive) are free.
    let has_linear_passage = || {
        assert!(is_straight_move || is_diagonal_move);
        let direction = (d_row.signum(), d_col.signum());
        let mut pos = from + direction;
        while pos != to {
            if grid[pos].is_some() {
                return false;
            }
            pos = pos + direction;
        }
        true
    };
    let simple_linear_passage = || {
        if has_linear_passage() { Ok } else { Blocked }
    };

    match piece_kind {
        PieceKind::Pawn => {
            let dir_forward = direction_forward(force);
            let second_row = SubjectiveRow::from_one_based(2).to_row(force);
            let valid_capturing_move = d_col.abs() == 1 && d_row == dir_forward;
            let valid_non_capturing_move = d_col == 0 && (
                d_row == dir_forward ||
                (from.row == second_row && d_row == dir_forward * 2)
            );
            match (valid_capturing_move, valid_non_capturing_move) {
                (true, true) => panic!("A pawn move cannot be both capturing and non-capturing"),
                (true, false) => OkIfCapturing,
                // TODO: Test that linear passage is verified for pawns too.
                (false, true) => if has_linear_passage() { OkIfNonCapturing } else { Blocked },
                (false, false) => Impossible,
            }
        },
        PieceKind::Knight => {
            if sort_two((d_row.abs(), d_col.abs())) == (1, 2) { Ok } else { Impossible }
        },
        PieceKind::Bishop => {
            if is_diagonal_move { simple_linear_passage() } else { Impossible }
        },
        PieceKind::Rook => {
            if is_straight_move { simple_linear_passage() } else { Impossible }
        },
        PieceKind::Queen => {
            if is_straight_move || is_diagonal_move { simple_linear_passage() } else { Impossible }
        },
        PieceKind::King => {
            if d_row.abs() <= 1 && d_col.abs() <= 1 { Ok } else { Impossible }
        },
    }
}

fn initial_castling_rights(grid: &Grid, force: Force) -> CastlingRights {
    let row = SubjectiveRow::from_one_based(1).to_row(force);
    let king_pos = find_king(grid, force).unwrap();
    assert!(king_pos.row == row);
    let mut rights = enum_map!{ _ => None };
    for col in Col::all() {
        if let Some(piece) = grid[Coord::new(row, col)] {
            if piece.kind == PieceKind::Rook && piece.force == force {
                use CastleDirection::*;
                let dir = if col < king_pos.col { ASide } else { HSide };
                assert!(rights[dir].is_none());
                rights[dir] = Some(col);
            }
        }
    }
    rights
}

fn remove_castling_right(castling_rights: &mut CastlingRights, col: Col) {
    for (_, col_rights) in castling_rights.iter_mut() {
        if *col_rights == Some(col) {
            *col_rights = None;
        }
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ProtoReachability {
    Ok,
    OkIfCapturing,
    OkIfNonCapturing,
    Blocked,
    Impossible,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reachability {
    Ok,
    Blocked,
    Impossible,
}

#[derive(Clone, Debug)]
struct TurnOutcome {
    new_grid: Grid,
    capture: Option<Capture>,
}

#[derive(Clone, Copy, Debug)]
pub struct Capture {
    piece_kind: PieceKind,
    force: Force,
}

// Note. Generally speaking, it's impossible to detect castling based on king movement in Chess960.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Turn {
    Move(TurnMove),
    Drop(TurnDrop),
    Castle(CastleDirection),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct TurnMove {
    pub from: Coord,
    pub to: Coord,
    pub promote_to: Option<PieceKind>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct TurnDrop {
    pub piece_kind: PieceKind,
    pub to: Coord,
}

// Turn, as entered by user.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TurnInput {
    Explicit(Turn),
    Algebraic(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnMode {
    // Regular in-order turn.
    Normal,

    // Out-of-order turn scheduled for execution. This is normally called "premove",
    // but we reserve "move" for a turn that takes one piece from the board and moves
    // it to another place on the board.
    //
    // A single preturn puts the game into an irrecoverably broken stake and should
    // never be executed on the main copy of the game.
    //
    // Assumptions for preturn:
    //   - Opponent pieces may have been removed, relocated or added.
    //   - Current player pieces may have been removed, but NOT relocated or added.
    // Validity test for a preturn is a strict as possible given these assuptions,
    // but not stricter.
    //
    // TODO: Classify TurnError-s into those that are ok for a pre-turn and those that
    // and not; test that a preturn is rejected iff the error is irrecoverable.
    Preturn,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VictoryReason {
    Checkmate,
    Flag,
    Resignation,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DrawReason {
    SimultaneousFlag,  // for bughouse
    ThreefoldRepetition,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChessGameStatus {
    Active,
    Victory(Force, VictoryReason),
    Draw(DrawReason),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnError {
    InvalidNotation,
    AmbiguousNotation,
    CaptureNotationRequiresCapture,
    PieceMissing,
    WrongTurnOrder,
    ImpossibleTrajectory,
    PathBlocked,
    UnprotectedKing,
    CastlingPieceHasMoved,
    BadPromotion,
    DropFobidden,
    DropPieceMissing,
    DropPosition,
    DropBlocked,
    DropAggression,
    GameOver,
}

pub type Reserve = EnumMap<PieceKind, u8>;

type CastlingRights = EnumMap<CastleDirection, Option<Col>>;

// In classic chess, positions are compared for threefold repetition using FIDE rules:
//
//   Two positions are by definition "the same" if the same types of pieces occupy the same
//   squares, the same player has the move, the remaining castling rights are the same and
//   the possibility to capture en passant is the same.
//
// For bughouse the total number of drops is included in addition. This effectively resets
// the counter every time a piece is dropped. Note that it could potentially lead to an
// infinite exchange loop involving both boards. But, given how unlikely this outcome is,
// it seems better than not having this rule.
//
// Improvement potential. Add rules to detect infinite loops involving both boards.
// Improvement potential. If this becomes a performance bottleneck, we could remove
// `total_drops` and instead clear the position set after every drop (as well as every
// capture, castling and pawn move).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct PositionForRepetitionDraw {
    grid: GridForRepetitionDraw,
    active_force: Force,
    castling_rights: EnumMap<Force, CastlingRights>,
    en_passant_target: Option<Coord>,
    total_drops: u32,
}

impl Reachability {
    pub fn ok(self) -> bool { self == Reachability::Ok }
}

// Improvement potential: Rc => references to a Box in Game classes
// Improvement potential: Don't players here since they don't affect gaem process.
#[derive(Clone, Debug)]
pub struct Board {
    chess_rules: Rc<ChessRules>,
    bughouse_rules: Option<Rc<BughouseRules>>,
    players: EnumMap<Force, Rc<PlayerInGame>>,
    status: ChessGameStatus,
    grid: Grid,
    // Tracks castling availability based on which pieces have moved. Castling is
    // allowed with the rooks stand in the first row at specified columns. If the
    // king has moved then the list is empty. Not affected by temporary limitations
    // (e.g. the king being checked).
    castling_rights: EnumMap<Force, CastlingRights>,
    en_passant_target: Option<Coord>,
    reserves: EnumMap<Force, Reserve>,
    total_drops: u32,  // total number of drops from both sides
    position_count: HashMap<PositionForRepetitionDraw, u32>,
    clock: Clock,
    active_force: Force,
}

impl Board {
    pub fn new(
        chess_rules: Rc<ChessRules>,
        bughouse_rules: Option<Rc<BughouseRules>>,
        players: EnumMap<Force, Rc<PlayerInGame>>,
        starting_grid: Grid,
    ) -> Board {
        let time_control = chess_rules.time_control.clone();
        let castling_rights = EnumMap::from_iter(
            Force::iter().map(|force| {
                (force, initial_castling_rights(&starting_grid, force))
            })
        );
        let mut board = Board {
            chess_rules,
            bughouse_rules,
            players,
            status: ChessGameStatus::Active,
            grid: starting_grid,
            castling_rights,
            en_passant_target: None,
            reserves: enum_map!{ _ => enum_map!{ _ => 0 } },
            total_drops: 0,
            position_count: HashMap::new(),
            clock: Clock::new(time_control),
            active_force: Force::White,
        };
        board.log_position_for_repetition_draw();
        board
    }

    pub fn chess_rules(&self) -> &Rc<ChessRules> { &self.chess_rules }
    pub fn bughouse_rules(&self) -> &Option<Rc<BughouseRules>> { &self.bughouse_rules }
    pub fn player(&self, force: Force) -> &PlayerInGame { &*self.players[force] }
    pub fn players(&self) -> &EnumMap<Force, Rc<PlayerInGame>> { &self.players }
    pub fn status(&self) -> ChessGameStatus { self.status }
    pub fn grid(&self) -> &Grid { &self.grid }
    pub fn grid_mut(&mut self) -> &mut Grid { &mut self.grid }
    pub fn castling_rights(&self) -> &EnumMap<Force, CastlingRights> { &self.castling_rights }
    pub fn en_passant_target(&self) -> Option<Coord> { self.en_passant_target }
    pub fn reserve(&self, force: Force) -> &Reserve { &self.reserves[force] }
    pub fn reserve_mut(&mut self, force: Force) -> &mut Reserve { &mut self.reserves[force] }
    pub fn reserves(&self) -> &EnumMap<Force, Reserve> { &self.reserves }
    pub fn clock(&self) -> &Clock { &self.clock }
    pub fn clock_mut(&mut self) -> &mut Clock { &mut self.clock }
    pub fn active_force(&self) -> Force { self.active_force }

    fn is_bughouse(&self) -> bool { self.bughouse_rules.is_some() }
    fn turn_owner(&self, mode: TurnMode) -> Force {
        match mode {
            TurnMode::Normal => self.active_force,
            TurnMode::Preturn => self.active_force.opponent(),
        }
    }

    pub fn start_clock(&mut self, now: GameInstant) {
        if !self.clock.is_active() {
            self.clock.new_turn(self.active_force, now);
        }
    }
    pub fn test_flag(&mut self, now: GameInstant) {
        if self.status != ChessGameStatus::Active {
            return;
        }
        if self.clock.time_left(self.active_force, now).is_zero() {
            self.status = ChessGameStatus::Victory(self.active_force.opponent(), VictoryReason::Flag);
        }
    }

    // Does not test flag. Will not update game status if a player has zero time left.
    pub fn try_turn(&mut self, turn: Turn, mode: TurnMode, now: GameInstant)
        -> Result<Option<Capture>, TurnError>
    {
        // Turn application is split into two phases:
        //   - First, check turn validity and determine the outcome (does not change
        //     game state, can fail if the turn is invalid).
        //   - Second, apply the outcome (changes game state, cannot fail).
        let TurnOutcome{ new_grid, capture } = self.turn_outcome(turn, mode)?;
        self.apply_turn(turn, mode, new_grid, capture, now);
        Ok(capture)
    }

    pub fn parse_turn_input(&self, turn_input: &TurnInput, mode: TurnMode) -> Result<Turn, TurnError> {
        Ok(match turn_input {
            TurnInput::Explicit(turn) => *turn,
            TurnInput::Algebraic(notation) => self.algebraic_to_turn(&notation, mode)?,
        })
    }

    fn log_position_for_repetition_draw(&mut self) {
        let position_for_repetition_draw = PositionForRepetitionDraw {
            grid: self.grid.map(|piece| {
                PieceForRepetitionDraw{ kind: piece.kind, force: piece.force }
            }),
            active_force: self.active_force,
            castling_rights: self.castling_rights,
            en_passant_target: self.en_passant_target,
            total_drops: self.total_drops,
        };
        let num_repetition = self.position_count.entry(position_for_repetition_draw).or_insert(0);
        *num_repetition += 1;
        if *num_repetition >= 3 {
            self.status = ChessGameStatus::Draw(DrawReason::ThreefoldRepetition);
        }
    }

    fn apply_turn(
        &mut self, turn: Turn, mode: TurnMode, new_grid: Grid, capture: Option<Capture>, now: GameInstant
    ) {
        let force = self.turn_owner(mode);
        match &turn {
            Turn::Move(mv) => {
                let first_row = SubjectiveRow::from_one_based(1).to_row(force);
                let piece = &mut self.grid[mv.from].unwrap();
                if piece.kind == PieceKind::King {
                    self.castling_rights[force].clear();
                } else if piece.kind == PieceKind::Rook && mv.from.row == first_row {
                    remove_castling_right(&mut self.castling_rights[force], mv.from.col);
                } else if let Some(capture) = capture {
                    let opponent = force.opponent();
                    assert_eq!(capture.force, opponent);
                    let opponent_first_row = SubjectiveRow::from_one_based(1).to_row(opponent);
                    if mv.to.row == opponent_first_row && capture.piece_kind == PieceKind::Rook {
                        remove_castling_right(&mut self.castling_rights[opponent], mv.to.col);
                    }
                }
            },
            Turn::Drop(_) => {
                self.total_drops += 1;
            },
            Turn::Castle(_) => {
                self.castling_rights[force].clear();
            }
        }
        self.grid = new_grid;
        if let Turn::Drop(drop) = turn {
            let reserve_left = &mut self.reserves[force][drop.piece_kind];
            assert!(*reserve_left > 0);
            *reserve_left -= 1;
        }

        match mode {
            TurnMode::Normal => {
                self.en_passant_target = get_en_passant_target(&self.grid, turn);
                let opponent_king_pos = find_king(&self.grid, force.opponent()).unwrap();
                if self.is_bughouse() {
                    if is_bughouse_mate_to(&mut self.grid, opponent_king_pos, self.en_passant_target) {
                        self.status = ChessGameStatus::Victory(force, VictoryReason::Checkmate);
                    }
                } else {
                    if is_chess_mate_to(&mut self.grid, opponent_king_pos, self.en_passant_target) {
                        self.status = ChessGameStatus::Victory(force, VictoryReason::Checkmate);
                    }
                }
                self.active_force = force.opponent();
                self.clock.new_turn(self.active_force, now);
                self.log_position_for_repetition_draw();
            },
            TurnMode::Preturn => {
                self.en_passant_target = None;
            },
        }
    }

    fn turn_outcome(&self, turn: Turn, mode: TurnMode) -> Result<TurnOutcome, TurnError> {
        let mut outcome = self.turn_outcome_no_check_test(turn, mode)?;
        match mode {
            TurnMode::Normal => { self.verify_check_and_drop_aggression(turn, mode, &mut outcome)? },
            TurnMode::Preturn => {},
        }
        Ok(outcome)
    }

    // `outcome` is guaratneed to be returned intact.
    fn verify_check_and_drop_aggression(&self, turn: Turn, mode: TurnMode, outcome: &mut TurnOutcome)
        -> Result<(), TurnError>
    {
        let new_grid = &mut outcome.new_grid;
        let force = self.turn_owner(mode);
        let king_pos = find_king(new_grid, force).unwrap();
        let opponent_king_pos = find_king(new_grid, force.opponent()).unwrap();
        if is_check_to(new_grid, king_pos) {
            return Err(TurnError::UnprotectedKing);
        }
        if let Turn::Drop(_) = turn {
            let bughouse_rules = self.bughouse_rules.as_ref().unwrap();  // should've been tested earlier
            let drop_legal = match bughouse_rules.drop_aggression {
                DropAggression::NoCheck =>
                    !is_check_to(new_grid, opponent_king_pos),
                DropAggression::NoChessMate =>
                    !is_chess_mate_to(new_grid, opponent_king_pos, self.en_passant_target),
                DropAggression::NoBughouseMate =>
                    !is_bughouse_mate_to(new_grid, opponent_king_pos, self.en_passant_target),
                DropAggression::MateAllowed =>
                    true,
            };
            if !drop_legal {
                return Err(TurnError::DropAggression);
            }
        }
        Ok(())
    }

    fn turn_outcome_no_check_test(&self, turn: Turn, mode: TurnMode)
        -> Result<TurnOutcome, TurnError>
    {
        if self.status != ChessGameStatus::Active {
            return Err(TurnError::GameOver);
        }
        let force = self.turn_owner(mode);
        let mut new_grid = self.grid.clone();
        let mut capture = None;
        match turn {
            Turn::Move(mv) => {
                let piece = new_grid[mv.from].ok_or(TurnError::PieceMissing)?;
                if piece.force != force {
                    return Err(TurnError::WrongTurnOrder);
                }
                let mut capture_pos_or = None;
                match mode {
                    TurnMode::Normal => {
                        use Reachability::*;
                        capture_pos_or = get_capture(&new_grid, mv.from, mv.to, self.en_passant_target);
                        match reachability(&new_grid, mv.from, mv.to, capture_pos_or.is_some()) {
                            Ok => {},
                            Blocked => return Err(TurnError::PathBlocked),
                            Impossible => return Err(TurnError::ImpossibleTrajectory),
                        }
                    },
                    TurnMode::Preturn => {
                        if !is_reachable_for_premove(&new_grid, mv.from, mv.to) {
                            return Err(TurnError::ImpossibleTrajectory);
                        }
                    },
                }
                new_grid[mv.from] = None;
                if let Some(capture_pos) = capture_pos_or {
                    let captured_piece = new_grid[capture_pos].unwrap();
                    capture = Some(Capture {
                        piece_kind: match captured_piece.origin {
                            PieceOrigin::Promoted => PieceKind::Pawn,
                            _ => captured_piece.kind,
                        },
                        force: captured_piece.force
                    });
                    new_grid[capture_pos] = None;
                }
                if should_promote(force, piece.kind, mv.to) {
                    if let Some(promote_to) = mv.promote_to {
                        if can_promote_to(promote_to) {
                            new_grid[mv.to] = Some(PieceOnBoard::new(
                                promote_to, PieceOrigin::Promoted, force
                            ));
                        } else {
                            return Err(TurnError::BadPromotion);
                        }
                    } else {
                        return Err(TurnError::BadPromotion);
                    }
                } else {
                    if let Some(_) = mv.promote_to {
                        return Err(TurnError::BadPromotion);
                    } else {
                        new_grid[mv.to] = Some(piece);
                    }
                }
            },
            Turn::Drop(drop) => {
                let bughouse_rules = self.bughouse_rules.as_ref().ok_or(TurnError::DropFobidden)?;
                let to_subjective_row = SubjectiveRow::from_row(drop.to.row, force);
                if drop.piece_kind == PieceKind::Pawn && (
                    to_subjective_row < bughouse_rules.min_pawn_drop_row ||
                    to_subjective_row > bughouse_rules.max_pawn_drop_row
                ) {
                    return Err(TurnError::DropPosition);
                }
                // Improvement potential: Allow pre-turns dropping missing pieces.
                if self.reserves[force][drop.piece_kind] < 1 {
                    return Err(TurnError::DropPieceMissing);
                }
                match mode {
                    TurnMode::Normal => {
                        if new_grid[drop.to].is_some() {
                            return Err(TurnError::DropBlocked);
                        }
                    },
                    TurnMode::Preturn => {},
                }
                new_grid[drop.to] = Some(PieceOnBoard::new(
                    drop.piece_kind, PieceOrigin::Dropped, force
                ));
            },
            Turn::Castle(dir) => {
                // TODO: More castling tests. Include cases:
                //   - Castle successful.
                //   - Cannot castle when king has moved.
                //   - Cannot castle when rook has moved.
                //   - Cannot castle when there are pieces in between.
                //   - King cannot starts in a checked square.
                //   - King cannot pass through a checked square.
                //   - King cannot ends up in a checked square.
                //   - Cannot castle if rook was captured and another one was
                //     dropped on its place.
                //   - [Chess960] Castle blocked by a piece at the destination,
                //      which is outside of kind and rook initial positions.
                //   - [Chess960] Castle when both rooks are on the same side,
                //      both when it's possible (the other rook is further away)
                //      and impossible (the other rook is in the way).

                let row = SubjectiveRow::from_one_based(1).to_row(force);
                // King can be missing in case of pre-turns.
                let king_from = find_king(&new_grid, force).ok_or(TurnError::CastlingPieceHasMoved)?;
                if king_from.row != row {
                    return Err(TurnError::CastlingPieceHasMoved);
                }
                let king = new_grid[king_from].take();

                let rook_col = self.castling_rights[force][dir].ok_or(TurnError::CastlingPieceHasMoved)?;
                let rook_from = Coord::new(row, rook_col);
                let rook = new_grid[rook_from].take();
                assert!(matches!(rook, Some(PieceOnBoard{ kind: PieceKind::Rook, .. })));

                let king_to;
                let rook_to;
                match dir {
                    CastleDirection::ASide => {
                        king_to = Coord::new(row, Col::C);
                        rook_to = Coord::new(row, Col::D);
                    },
                    CastleDirection::HSide => {
                        king_to = Coord::new(row, Col::G);
                        rook_to = Coord::new(row, Col::F);
                    },
                };

                match mode {
                    TurnMode::Normal => {
                        let cols = [king_from.col, king_to.col, rook_from.col, rook_to.col];
                        for col in col_range_inclusive(iter_minmax(cols.into_iter()).unwrap()) {
                            if new_grid[Coord::new(row, col)].is_some() {
                                return Err(TurnError::PathBlocked);
                            }
                        }

                        let cols = [king_from.col, king_to.col];
                        for col in col_range_inclusive(iter_minmax(cols.into_iter()).unwrap()) {
                            let pos = Coord::new(row, col);
                            let new_grid = new_grid.scoped_set(pos, king);
                            if is_check_to(&new_grid, pos) {
                                return Err(TurnError::UnprotectedKing);
                            }
                        }
                    },
                    TurnMode::Preturn => {},
                }

                new_grid[king_to] = king;
                new_grid[rook_to] = rook;
            },
        }
        Ok(TurnOutcome{ new_grid, capture })
    }

    pub fn receive_capture(&mut self, capture: &Capture) {
        self.reserves[capture.force][capture.piece_kind] += 1;
    }

    pub fn algebraic_to_turn(&self, notation: &str, mode: TurnMode) -> Result<Turn, TurnError> {
        let force = self.turn_owner(mode);
        let notation = notation.trim();
        const PIECE_RE: &str = r"[PNBRQK]";
        lazy_static! {
            static ref MOVE_RE: Regex = Regex::new(
                &format!(r"^({piece})?([a-h])?([1-8])?([x×:])?([a-h][1-8])(?:[=/]?({piece})?)([+†#‡]?)$", piece=PIECE_RE)
            ).unwrap();
            static ref DROP_RE: Regex = Regex::new(
                &format!(r"^({piece})@([a-h][1-8])$", piece=PIECE_RE)
            ).unwrap();
            static ref A_CASTLING_RE: Regex = Regex::new("^(0-0-0|O-O-O)$").unwrap();
            static ref H_CASTLING_RE: Regex = Regex::new("^(0-0|O-O)$").unwrap();
        }
        if let Some(cap) = MOVE_RE.captures(notation) {
            let piece_kind = cap.get(1).map_or(PieceKind::Pawn, |m| PieceKind::from_algebraic(m.as_str()).unwrap());
            let from_col = cap.get(2).map(|m| Col::from_algebraic(as_single_char(m.as_str()).unwrap()));
            let from_row = cap.get(3).map(|m| Row::from_algebraic(as_single_char(m.as_str()).unwrap()));
            let capturing = cap.get(4).is_some();
            let to = Coord::from_algebraic(cap.get(5).unwrap().as_str());
            let promote_to = cap.get(6).map(|m| PieceKind::from_algebraic(m.as_str()).unwrap());
            let _mark = cap.get(7).map(|m| m.as_str());  // TODO: Test check/mate
            if promote_to.is_some() != should_promote(force, piece_kind, to) {
                return Err(TurnError::BadPromotion);
            }
            let mut turn = None;
            let mut potentially_reachable = false;
            for from in Coord::all() {
                if let Some(piece) = self.grid[from] {
                    if (
                        piece.force == force &&
                        piece.kind == piece_kind &&
                        from_row.unwrap_or(from.row) == from.row &&
                        from_col.unwrap_or(from.col) == from.col
                    ) {
                        let reachable;
                        match mode {
                            TurnMode::Normal => {
                                use Reachability::*;
                                let capture_or = get_capture(&self.grid, from, to, self.en_passant_target);
                                match reachability(&self.grid, from, to, capture_or.is_some()) {
                                    Ok => {
                                        if capturing && !capture_or.is_some() {
                                            return Err(TurnError::CaptureNotationRequiresCapture);
                                        }
                                        reachable = true;
                                    },
                                    Blocked => {
                                        potentially_reachable = true;
                                        reachable = false;
                                    },
                                    Impossible => {
                                        reachable = false;
                                    },
                                }
                            },
                            TurnMode::Preturn => {
                                reachable = is_reachable_for_premove(&self.grid, from, to)
                            },
                        };
                        if reachable {
                            if turn.is_some() {
                                // Note. Checking for a preturn may reject a turn that would
                                // become valid by the time it's executed (because one of the
                                // pieces that can make the move is blocked or captured, so
                                // it's no longer ambiguous). However without this condition
                                // it is unclear how to render the preturn on the client.
                                return Err(TurnError::AmbiguousNotation);
                            }
                            turn = Some(Turn::Move(TurnMove{ from, to, promote_to }));
                        }
                    }
                }
            }
            if let Some(turn) = turn {
                return Ok(turn);
            } else if potentially_reachable {
                return Err(TurnError::PathBlocked);
            } else {
                return Err(TurnError::ImpossibleTrajectory);
            }
        } else if let Some(cap) = DROP_RE.captures(notation) {
            let piece_kind = PieceKind::from_algebraic(cap.get(1).unwrap().as_str()).unwrap();
            let to = Coord::from_algebraic(cap.get(2).unwrap().as_str());
            return Ok(Turn::Drop(TurnDrop{ piece_kind, to }));
        } else if A_CASTLING_RE.is_match(notation) {
            return Ok(Turn::Castle(CastleDirection::ASide));
        } else if H_CASTLING_RE.is_match(notation) {
            return Ok(Turn::Castle(CastleDirection::HSide));
        }
        Err(TurnError::InvalidNotation)
    }

    // Renders turn as algebraic notation, PGN-style, see
    //   http://www.saremba.de/chessgml/standards/pgn/pgn-complete.htm
    // Improvement potential. Omit `from` square when possible.
    // Improvement potential. Allow to choose formatting style (e.g. "0-0" or "O-O").
    pub fn turn_to_algebraic(&self, turn: Turn) -> Result<String, TurnError> {
        let notation = self.turn_to_algebraic_impl(turn)?;
        // Improvement potential. Remove when sufficiently tested.
        if let Ok(turn_parsed) = self.algebraic_to_turn(&notation, TurnMode::Normal) {
            assert_eq!(turn_parsed, turn, "{}", notation);
        }
        Ok(notation)
    }

    fn turn_to_algebraic_impl(&self, turn: Turn) -> Result<String, TurnError> {
        match turn {
            Turn::Move(mv) => {
                let piece = self.grid[mv.from].ok_or(TurnError::PieceMissing)?;
                let capture = get_capture(&self.grid, mv.from, mv.to, self.en_passant_target);
                let promotion = match mv.promote_to {
                    Some(piece_kind) => format!("={}", piece_kind.to_full_algebraic()),
                    None => "".to_owned(),
                };
                Ok(format!(
                    "{}{}{}{}{}",
                    piece.kind.to_algebraic_for_move(),
                    mv.from.to_algebraic(),
                    if capture.is_some() { "x" } else { "" },
                    mv.to.to_algebraic(),
                    promotion,
                ))
            },
            Turn::Drop(drop) => {
                Ok(format!("{}@{}", drop.piece_kind.to_full_algebraic(), drop.to.to_algebraic()))
            },
            Turn::Castle(dir) => {
                Ok((match dir {
                    CastleDirection::ASide => "O-O-O",
                    CastleDirection::HSide => "O-O",
                }).to_owned())
            },
        }
    }
}
