// Rust-upgrade (https://github.com/rust-lang/rust/issues/46379):
//   remove `#[allow(dead_code)]` before public functions.
//
// Improvement potential. Combine integration tests together:
//   https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

use std::rc::Rc;

use bughouse_chess::test_util::sample_chess_players;
use bughouse_chess::util::as_single_char;
use bughouse_chess::*;
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
            force: bughouse_chess::PieceForce::$force,
            kind: bughouse_chess::PieceKind::$kind,
        }
    };
}


pub trait AutoTurnInput {
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
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::Move(bughouse_chess::TurnMove {
            from: bughouse_chess::Coord::$from,
            to: bughouse_chess::Coord::$to,
            promote_to: None,
        }))
    };
    ($from:ident -> $to:ident = $steal_piece_kind:ident $steal_piece_id:ident) => {
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::Move(bughouse_chess::TurnMove {
            from: bughouse_chess::Coord::$from,
            to: bughouse_chess::Coord::$to,
            promote_to: Some(PromotionTarget::Steal((
                bughouse_chess::PieceKind::$steal_piece_kind,
                $steal_piece_id,
            ))),
        }))
    };
    ($piece_kind:ident @ $to:ident) => {
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::Drop(bughouse_chess::TurnDrop {
            piece_kind: bughouse_chess::PieceKind::$piece_kind,
            to: bughouse_chess::Coord::$to,
        }))
    };
    (@ $to:ident) => {
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::PlaceDuck(
            bughouse_chess::Coord::$to,
        ))
    };
}

#[allow(dead_code)]
pub fn algebraic_turn(algebraic: &str) -> TurnInput {
    bughouse_chess::TurnInput::Algebraic(algebraic.to_owned())
}


#[macro_export]
macro_rules! envoy {
    ($force:ident $board_idx:ident) => {
        bughouse_chess::BughouseEnvoy {
            board_idx: bughouse_chess::BughouseBoard::$board_idx,
            force: bughouse_chess::Force::$force,
        }
    };
}

#[allow(dead_code)]
pub fn parse_board(rules: Rules, board_str: &str) -> Result<Board, String> {
    let board_shape = rules.chess_rules.board_shape();
    let rows = board_str
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.split_ascii_whitespace().collect_vec())
        .collect_vec();
    assert_eq!(rows.len(), board_shape.num_rows as usize);
    assert!(rows.iter().all(|row| row.len() == board_shape.num_cols as usize));
    let mut grid = Grid::new(board_shape);
    for (row_idx, row) in rows.iter().rev().enumerate() {
        for (col_idx, piece_str) in row.iter().enumerate() {
            let piece_char =
                as_single_char(piece_str).ok_or_else(|| format!("Invalid piece: {}", piece_str))?;
            let coord = Coord::new(
                Row::from_zero_based(row_idx as i8),
                Col::from_zero_based(col_idx as i8),
            );
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
    let castling_rights = enum_map! { _ => enum_map! { _ => None } };
    let players = sample_chess_players();
    Ok(Board::new_from_grid(
        Rc::new(rules),
        players,
        grid,
        next_piece_id,
        castling_rights,
    ))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn strip_piece_ids(grid: &Grid) -> Grid {
        grid.map(|p| PieceOnBoard { id: PieceId::tmp(), ..p })
    }

    #[test]
    fn parse_board_opening() {
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
        let board = parse_board(rules.clone(), board_str).unwrap();

        let mut game = ChessGame::new(rules, sample_chess_players());
        game.try_turn(&algebraic_turn("e4"), TurnMode::Normal, T0).unwrap();
        game.try_turn(&algebraic_turn("d5"), TurnMode::Normal, T0).unwrap();
        let board_expected = game.board();

        assert_eq!(strip_piece_ids(board.grid()), strip_piece_ids(board_expected.grid()));
    }
}
