// Rust-upgrade (https://github.com/rust-lang/rust/issues/46379):
//   remove `#[allow(dead_code)]` before public functions.
//
// Improvement potential. Combine integration tests together:
//   https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html
//
// Improvement potential. Unit tests in this file are executed by each test module. To fix this we
//   could combine all integration tests together (see above) or move out the unit tests.

use std::collections::HashMap;
use std::rc::Rc;

use bughouse_chess::board::{Board, Reserve, TurnInput};
use bughouse_chess::display::{from_display_coord, BoardOrientation, DisplayCoord};
use bughouse_chess::force::Force;
use bughouse_chess::game::{BughouseBoard, BughouseGame};
use bughouse_chess::grid::Grid;
use bughouse_chess::once_cell_regex;
use bughouse_chess::piece::{
    piece_from_ascii, PieceForce, PieceId, PieceKind, PieceOnBoard, PieceOrigin,
};
use bughouse_chess::role::Role;
use bughouse_chess::rules::Rules;
use bughouse_chess::starter::{assign_piece_ids, BoardSetup, EffectiveStartingPosition};
use bughouse_chess::test_util::{sample_bughouse_players, sample_chess_players};
use bughouse_chess::util::as_single_char;
use enum_map::enum_map;
use itertools::Itertools;


#[derive(Clone, Copy, Debug)]
pub struct PieceMatcher {
    pub kind: PieceKind,
    pub force: PieceForce,
}

pub trait PieceIs {
    fn is(self, matcher: PieceMatcher) -> bool;
}

impl PieceIs for Option<PieceOnBoard> {
    fn is(self, matcher: PieceMatcher) -> bool {
        if let Some(piece) = self {
            piece.kind == matcher.kind && piece.force == matcher.force
        } else {
            false
        }
    }
}

#[macro_export]
macro_rules! piece {
    ($force:ident $kind:ident) => {
        common::PieceMatcher {
            force: bughouse_chess::piece::PieceForce::$force,
            kind: bughouse_chess::piece::PieceKind::$kind,
        }
    };
}


pub trait AutoTurnInput {
    #[allow(dead_code)]
    fn to_turn_input(self) -> TurnInput;
}

impl AutoTurnInput for &str {
    fn to_turn_input(self) -> TurnInput { TurnInput::Algebraic(self.to_owned()) }
}

impl AutoTurnInput for TurnInput {
    fn to_turn_input(self) -> TurnInput { self }
}

#[macro_export]
macro_rules! drag_move {
    ($from:ident -> $to:ident) => {
        bughouse_chess::board::TurnInput::DragDrop(bughouse_chess::board::Turn::Move(
            bughouse_chess::board::TurnMove {
                from: bughouse_chess::coord::Coord::$from,
                to: bughouse_chess::coord::Coord::$to,
                promote_to: None,
            },
        ))
    };
    ($from:ident -> $to:ident = $steal_piece:ident) => {
        bughouse_chess::board::TurnInput::DragDrop(bughouse_chess::board::Turn::Move(
            bughouse_chess::board::TurnMove {
                from: bughouse_chess::coord::Coord::$from,
                to: bughouse_chess::coord::Coord::$to,
                promote_to: Some(bughouse_chess::board::PromotionTarget::Steal((
                    $steal_piece.kind,
                    $steal_piece.origin,
                    $steal_piece.id,
                ))),
            },
        ))
    };
    ($piece_kind:ident @ $to:ident) => {
        bughouse_chess::board::TurnInput::DragDrop(bughouse_chess::board::Turn::Drop(
            bughouse_chess::board::TurnDrop {
                piece_kind: bughouse_chess::piece::PieceKind::$piece_kind,
                to: bughouse_chess::coord::Coord::$to,
            },
        ))
    };
    (@ $to:ident) => {
        bughouse_chess::board::TurnInput::DragDrop(bughouse_chess::board::Turn::PlaceDuck(
            bughouse_chess::coord::Coord::$to,
        ))
    };
}

#[allow(dead_code)]
pub fn algebraic_turn(algebraic: &str) -> TurnInput {
    bughouse_chess::board::TurnInput::Algebraic(algebraic.to_owned())
}


pub trait ReserveAsHashMap {
    #[allow(dead_code)]
    fn to_map(self) -> HashMap<PieceKind, u8>;
}

impl ReserveAsHashMap for Reserve {
    fn to_map(self) -> HashMap<PieceKind, u8> { self.into_iter().filter(|&(_, n)| n > 0).collect() }
}

pub trait GridExt {
    fn without_ids(&self) -> Self;
}

impl GridExt for Grid {
    fn without_ids(&self) -> Grid { self.map(|piece| PieceOnBoard { id: PieceId::tmp(), ..piece }) }
}

fn parse_ascii_setup<'a>(
    rules: &Rules, board_line: impl IntoIterator<Item = &'a str>,
    board_orientation: BoardOrientation,
) -> Result<BoardSetup, String> {
    let board_shape = rules.chess_rules.board_shape();
    let rows = board_line.into_iter().map(|line| line.split(' ').collect_vec()).collect_vec();
    assert_eq!(rows.len(), board_shape.num_rows as usize);
    assert!(rows.iter().all(|row| row.len() == board_shape.num_cols as usize));
    let mut grid = Grid::new(board_shape);
    for (y, row) in rows.iter().enumerate() {
        for (x, piece_str) in row.iter().enumerate() {
            let piece_char =
                as_single_char(piece_str).ok_or_else(|| format!("Invalid piece: {}", piece_str))?;
            let display_coord = DisplayCoord { x: x as i8, y: y as i8 };
            let coord = from_display_coord(display_coord, board_shape, board_orientation).unwrap();
            let piece = if piece_char == '.' {
                None
            } else {
                let (kind, force) = piece_from_ascii(piece_char)
                    .ok_or_else(|| format!("Invalid piece: {}", piece_char))?;
                Some(PieceOnBoard {
                    id: PieceId::tmp(),
                    kind,
                    origin: PieceOrigin::Innate,
                    force,
                })
            };
            grid[coord] = piece;
        }
    }
    let mut next_piece_id = PieceId::new();
    assign_piece_ids(&mut grid, &mut next_piece_id);
    Ok(BoardSetup {
        grid,
        next_piece_id,
        castling_rights: enum_map! { _ => enum_map! { _ => None } },
        en_passant_target: None,
        reserves: enum_map! { _ => enum_map!{ _ => 0 } },
        active_force: Force::White,
    })
}

// Parses board representation in ASCII format.
//   - Board squares must be separated by exactly one space.
//   - Pieces are denoted by their algebraic notations.
//   - Capital letter mean white pieces, small letter mean black pieces (as in FEN).
//   - Empty squares are denoted with '.'.
//   - Boards orientation: white is at the bottom, black is at the top.
//
// In the produced board:
//   - All pieces are considered innate (not promoted or dropped).
//   - Castling is forbidden.
//   - White is to move.
//
// For an example, see `parse_ascii_board_opening` test.
#[allow(dead_code)]
pub fn parse_ascii_board(rules: Rules, role: Role, board_str: &str) -> Result<Board, String> {
    let lines = board_str.split('\n').map(|line| line.trim()).filter(|line| !line.is_empty());
    let setup = parse_ascii_setup(&rules, lines, BoardOrientation::Normal)?;
    let players = sample_chess_players();
    Ok(Board::new_from_setup(Rc::new(rules), role, players, setup))
}

// Parses board representation in ASCII format.
//   - Boards must be separated by two or more spaces.
//   - Boards orientation: white is at the bottom left and top right, black is at the top left and
//     bottom right.
//   - ... otherwise the format follows that of `parse_ascii_board`.
//
// In the produced game:
//   - All pieces are considered innate (not promoted or dropped).
//   - Reserves are empty.
//   - Castling is forbidden.
//   - White is to move on both boards.
//   - Board A is the left one, board B is the right one.
//
// For an example, see `parse_ascii_bughouse_opening` test.
#[allow(dead_code)]
pub fn parse_ascii_bughouse(
    rules: Rules, role: Role, game_str: &str,
) -> Result<BughouseGame, String> {
    let lots_of_spaces = once_cell_regex!(r" {2,}");
    let (lines_a, lines_b): (Vec<_>, Vec<_>) = game_str
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| lots_of_spaces.split(line).collect_tuple().unwrap())
        .unzip();
    let mut setup = HashMap::new();
    setup.insert(BughouseBoard::A, parse_ascii_setup(&rules, lines_a, BoardOrientation::Normal)?);
    setup.insert(BughouseBoard::B, parse_ascii_setup(&rules, lines_b, BoardOrientation::Rotated)?);
    let starting_position = EffectiveStartingPosition::ManualSetup(setup);
    let players = sample_bughouse_players();
    Ok(BughouseGame::new_with_starting_position(
        rules,
        role,
        starting_position,
        &players,
    ))
}

/*
    Copy-pasteable!

    r n b q k b n r     R N B K Q B N R
    p p p p p p p p     P P P P P P P P
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    P P P P P P P P     p p p p p p p p
    R N B Q K B N R     r n b k q b n r

    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
    . . . . . . . .     . . . . . . . .
*/

#[cfg(test)]
mod tests {
    use bughouse_chess::board::TurnMode;
    use bughouse_chess::clock::GameInstant;
    use bughouse_chess::game::ChessGame;
    use bughouse_chess::rules::{ChessRules, MatchRules};
    use strum::IntoEnumIterator;

    use super::*;

    #[test]
    fn parse_ascii_board_opening() {
        const T0: GameInstant = GameInstant::game_start();
        let rules = Rules {
            match_rules: MatchRules::unrated(),
            chess_rules: ChessRules::chess_blitz(),
        };
        let board_str = "
            r n b q k b n r
            p p p . p p p p
            . . . . . . . .
            . . . p . . . .
            . . . . P . . .
            . . . . . . . .
            P P P P . P P P
            R N B Q K B N R
        ";
        let board = parse_ascii_board(rules.clone(), Role::ServerOrStandalone, board_str).unwrap();

        let mut game_expected =
            ChessGame::new(rules, Role::ServerOrStandalone, sample_chess_players());
        game_expected.try_turn(&algebraic_turn("e4"), TurnMode::Normal, T0).unwrap();
        game_expected.try_turn(&algebraic_turn("d5"), TurnMode::Normal, T0).unwrap();
        let board_expected = game_expected.board();

        assert_eq!(board.grid().without_ids(), board_expected.grid().without_ids());
    }

    #[test]
    fn parse_ascii_bughouse_opening() {
        use BughouseBoard::*;
        const T0: GameInstant = GameInstant::game_start();
        let rules = Rules {
            match_rules: MatchRules::unrated(),
            chess_rules: ChessRules::bughouse_chess_com(),
        };
        let game_str = "
            r n b q k b n r     R N B K Q B N R
            p p p . p p p p     P P P . P P P P
            . . . . . . . .     . . . P . . . .
            . . . p . . . .     . . . . . . . .
            . . . . P . . .     . . . . . . . .
            . . . . . . . .     . . n . . . . .
            P P P P . P P P     p p p p p p p p
            R N B Q K B N R     r . b k q b n r
        ";
        let game = parse_ascii_bughouse(rules.clone(), Role::ServerOrStandalone, game_str).unwrap();

        let mut game_expected =
            BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
        game_expected.try_turn(A, &algebraic_turn("e4"), TurnMode::Normal, T0).unwrap();
        game_expected.try_turn(A, &algebraic_turn("d5"), TurnMode::Normal, T0).unwrap();
        game_expected.try_turn(B, &algebraic_turn("e3"), TurnMode::Normal, T0).unwrap();
        game_expected.try_turn(B, &algebraic_turn("Nf6"), TurnMode::Normal, T0).unwrap();

        for board_idx in BughouseBoard::iter() {
            assert_eq!(
                game.board(board_idx).grid().without_ids(),
                game_expected.board(board_idx).grid().without_ids()
            );
        }
    }
}
