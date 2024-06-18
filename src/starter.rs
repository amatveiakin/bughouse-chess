use std::collections::HashMap;

use enum_map::EnumMap;
use itertools::Itertools;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::board::{BoardCastlingRights, Reserve};
use crate::coord::{Col, Coord, Row};
use crate::force::Force;
use crate::game::BughouseBoard;
use crate::grid::Grid;
use crate::piece::{PieceForce, PieceId, PieceKind, PieceOnBoard, PieceOrigin};
use crate::rules::{ChessRules, FairyPieces, StartingPosition};


#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BoardSetup {
    pub grid: Grid,
    pub next_piece_id: PieceId,
    pub castling_rights: BoardCastlingRights,
    pub en_passant_target: Option<Coord>,
    pub reserves: EnumMap<Force, Reserve>,
    pub active_force: Force,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum EffectiveStartingPosition {
    Classic,
    FischerRandom(Vec<PieceKind>),
    // Not using EnumMap: it is inconvenient with complex non-copiable types.
    ManualSetup(HashMap<BughouseBoard, BoardSetup>),
}

impl EffectiveStartingPosition {
    pub fn manual_duplicate(board_setup: &BoardSetup) -> Self {
        EffectiveStartingPosition::ManualSetup({
            BughouseBoard::iter()
                .map(|board_idx| (board_idx, board_setup.clone()))
                .collect()
        })
    }
}

fn new_white(kind: PieceKind) -> PieceOnBoard {
    PieceOnBoard::new(PieceId::tmp(), kind, PieceOrigin::Innate, PieceForce::White)
}

fn setup_white_pawns_on_2nd_row(grid: &mut Grid) {
    for col in grid.shape().cols() {
        grid[Coord::new(Row::_2, col)] = Some(new_white(PieceKind::Pawn));
    }
}

fn setup_black_pieces_mirrorlike(grid: &mut Grid) {
    for coord in grid.shape().coords() {
        if let Some(piece) = grid[coord] {
            if piece.force == PieceForce::White {
                let mirror_row = Row::from_zero_based(
                    grid.shape().num_rows as i8 - coord.row.to_zero_based() - 1,
                );
                let mirror_coord = Coord::new(mirror_row, coord.col);
                assert!(grid[mirror_coord].is_none(), "{:?}", grid);
                grid[mirror_coord] = Some(PieceOnBoard { force: PieceForce::Black, ..piece });
            }
        }
    }
}

pub fn assign_piece_ids(grid: &mut Grid, piece_id: &mut PieceId) {
    for coord in grid.shape().coords() {
        if let Some(piece) = grid[coord] {
            grid[coord] = Some(PieceOnBoard { id: piece_id.inc(), ..piece });
        }
    }
}

pub fn generate_starting_position(rules: &ChessRules) -> EffectiveStartingPosition {
    use FairyPieces::*;
    use PieceKind::*;
    match (rules.fairy_pieces, rules.starting_position) {
        (_, StartingPosition::Classic) => EffectiveStartingPosition::Classic,
        (NoFairy | Accolade, StartingPosition::FischerRandom) => {
            assert_eq!(rules.board_shape().num_cols, 8);
            let mut rng = rand::thread_rng();
            let mut row = [None; 8];
            row[rng.gen_range(0..4) * 2] = Some(Bishop);
            row[rng.gen_range(0..4) * 2 + 1] = Some(Bishop);
            let mut cols = row
                .iter()
                .enumerate()
                .filter_map(|(col, piece)| if piece.is_none() { Some(col) } else { None })
                .collect_vec();
            cols.shuffle(&mut rng);
            let (king_and_rook_cols, queen_and_knight_cols) = cols.split_at(3);
            let (&left_rook_col, &king_col, &right_rook_col) =
                king_and_rook_cols.iter().sorted().collect_tuple().unwrap();
            let (&queen_col, &knight_col_1, &knight_col_2) =
                queen_and_knight_cols.iter().collect_tuple().unwrap();
            row[left_rook_col] = Some(Rook);
            row[king_col] = Some(King);
            row[right_rook_col] = Some(Rook);
            row[queen_col] = Some(Queen);
            row[knight_col_1] = Some(Knight);
            row[knight_col_2] = Some(Knight);
            EffectiveStartingPosition::FischerRandom(row.map(|col| col.unwrap()).into())
        }
        // TODO: Consider which other variants should support Fischer random.
        (Capablanca, StartingPosition::FischerRandom) => EffectiveStartingPosition::Classic,
    }
}

pub fn starting_piece_row(
    fairy_pieces: FairyPieces, starting_position: &EffectiveStartingPosition,
) -> &[PieceKind] {
    use EffectiveStartingPosition::*;
    use FairyPieces::*;
    use PieceKind::*;
    match (fairy_pieces, starting_position) {
        (NoFairy | Accolade, Classic) => &[Rook, Knight, Bishop, Queen, King, Bishop, Knight, Rook],
        (Capablanca, Classic) => &[
            Rook, Knight, Cardinal, Bishop, Queen, King, Bishop, Empress, Knight, Rook,
        ],
        (_, FischerRandom(row)) => row,
        (_, ManualSetup(_)) => {
            panic!("Must use Board::new_from_setup with EffectiveStartingPosition::ManualSetup")
        }
    }
}

pub fn generate_starting_grid(
    rules: &ChessRules, starting_position: &EffectiveStartingPosition, piece_id: &mut PieceId,
) -> Grid {
    let mut grid = Grid::new(rules.board_shape());
    let starting_row = &starting_piece_row(rules.fairy_pieces, starting_position);
    assert_eq!(starting_row.len(), grid.shape().num_cols as usize);
    for (col, piece_kind) in starting_row.iter().enumerate() {
        let coord = Coord::new(Row::_1, Col::from_zero_based(col.try_into().unwrap()));
        grid[coord] = Some(new_white(*piece_kind));
    }
    setup_white_pawns_on_2nd_row(&mut grid);
    setup_black_pieces_mirrorlike(&mut grid);
    assign_piece_ids(&mut grid, piece_id);
    grid
}
