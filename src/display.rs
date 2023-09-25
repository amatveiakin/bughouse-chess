// Improvement potential: Standardize naming.
// Improvement potential: Add tests verifying inverse and commutative relations.

use std::ops;

use serde::{Deserialize, Serialize};
use strum::EnumIter;

use crate::coord::{BoardShape, Col, Coord, Row};
use crate::force::Force;
use crate::game::{get_bughouse_board, BughouseBoard, BughouseParticipant, BughousePlayer};


#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
pub enum DisplayBoard {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
pub enum DisplayPlayer {
    Top,
    Bottom,
}

// Lens through which to view the game: the corresponding envoy will be rendered in
// bottom left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Perspective {
    pub board_idx: BughouseBoard,
    pub force: Force,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoardOrientation {
    Normal,  // White at bottom
    Rotated, // Black at bottom
}

// These coords describe board squares, like `Coord`. For a regular chess board,
// both `x` and `y` are integers between 0 and 7. But here row 0 corresponds to
// the top-most row, which could be row '1' or row '8' on the board.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayCoord {
    pub x: i8,
    pub y: i8,
}

// Floating-point coords associated with `Coord` coordinate system.
// Point (0., 0.) corresponds to the outer corner of 'a1' square, while
// point (8., 8.) corresponds to the outer corner of 'h8' square.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct FCoord {
    pub x: f64,
    pub y: f64,
}

// Floating-point coords associated with `DisplayCoord` coordinate system.
// Point (0., 0.) corresponds to the top left corner of the top left square, while
// point (8., 8.) corresponds to the bottom right corner of the bottom right square.
#[derive(Clone, Copy, Debug)]
pub struct DisplayFCoord {
    pub x: f64,
    pub y: f64,
}


impl Perspective {
    pub fn for_participant(participant: BughouseParticipant) -> Self {
        use BughousePlayer::*;
        match participant {
            BughouseParticipant::Player(SinglePlayer(envoy)) => Perspective {
                board_idx: envoy.board_idx,
                force: envoy.force,
            },
            BughouseParticipant::Player(DoublePlayer(team)) => Perspective {
                board_idx: get_bughouse_board(team, Force::White),
                force: Force::White,
            },
            BughouseParticipant::Observer => Perspective {
                board_idx: BughouseBoard::A,
                force: Force::White,
            },
        }
    }
}

pub fn get_board_index(board: DisplayBoard, perspective: Perspective) -> BughouseBoard {
    match board {
        DisplayBoard::Primary => perspective.board_idx,
        DisplayBoard::Secondary => perspective.board_idx.other(),
    }
}

pub fn get_display_board_index(board: BughouseBoard, perspective: Perspective) -> DisplayBoard {
    if perspective.board_idx == board {
        DisplayBoard::Primary
    } else {
        DisplayBoard::Secondary
    }
}

pub fn get_board_orientation(board: DisplayBoard, perspective: Perspective) -> BoardOrientation {
    use DisplayBoard::*;
    use Force::*;
    match (board, perspective.force) {
        (Primary, White) | (Secondary, Black) => BoardOrientation::Normal,
        (Primary, Black) | (Secondary, White) => BoardOrientation::Rotated,
    }
}

pub fn get_display_player(force: Force, orientation: BoardOrientation) -> DisplayPlayer {
    use BoardOrientation::*;
    use Force::*;
    match (orientation, force) {
        (Normal, White) | (Rotated, Black) => DisplayPlayer::Bottom,
        (Normal, Black) | (Rotated, White) => DisplayPlayer::Top,
    }
}

pub fn to_display_coord(
    coord: Coord, board_shape: BoardShape, orientation: BoardOrientation,
) -> DisplayCoord {
    match orientation {
        BoardOrientation::Normal => DisplayCoord {
            x: coord.col.to_zero_based(),
            y: board_shape.num_rows as i8 - coord.row.to_zero_based() - 1,
        },
        BoardOrientation::Rotated => DisplayCoord {
            x: board_shape.num_cols as i8 - coord.col.to_zero_based() - 1,
            y: coord.row.to_zero_based(),
        },
    }
}

pub fn to_display_fcoord(
    p: FCoord, board_shape: BoardShape, orientation: BoardOrientation,
) -> DisplayFCoord {
    match orientation {
        BoardOrientation::Normal => DisplayFCoord {
            x: p.x,
            y: (board_shape.num_rows as f64) - p.y,
        },
        BoardOrientation::Rotated => DisplayFCoord {
            x: (board_shape.num_cols as f64) - p.x,
            y: p.y,
        },
    }
}

pub fn from_display_row(
    y: i8, board_shape: BoardShape, orientation: BoardOrientation,
) -> Option<Row> {
    let row = match orientation {
        BoardOrientation::Normal => Row::from_zero_based(board_shape.num_rows as i8 - y - 1),
        BoardOrientation::Rotated => Row::from_zero_based(y),
    };
    board_shape.contains_row(row).then_some(row)
}

pub fn from_display_col(
    x: i8, board_shape: BoardShape, orientation: BoardOrientation,
) -> Option<Col> {
    let col = match orientation {
        BoardOrientation::Normal => Col::from_zero_based(x),
        BoardOrientation::Rotated => Col::from_zero_based(board_shape.num_cols as i8 - x - 1),
    };
    board_shape.contains_col(col).then_some(col)
}

pub fn from_display_coord(
    q: DisplayCoord, board_shape: BoardShape, orientation: BoardOrientation,
) -> Option<Coord> {
    Some(Coord {
        row: from_display_row(q.y, board_shape, orientation)?,
        col: from_display_col(q.x, board_shape, orientation)?,
    })
}

pub fn display_to_fcoord(
    q: DisplayFCoord, board_shape: BoardShape, orientation: BoardOrientation,
) -> FCoord {
    match orientation {
        BoardOrientation::Normal => FCoord {
            x: q.x,
            y: (board_shape.num_rows as f64) - q.y,
        },
        BoardOrientation::Rotated => FCoord {
            x: (board_shape.num_cols as f64) - q.x,
            y: q.y,
        },
    }
}

impl FCoord {
    // Returns the closes valid board square.
    pub fn to_coord_snapped(self, board_shape: BoardShape) -> Coord {
        Coord::new(
            Row::from_zero_based((self.y.clamp(0., (board_shape.num_rows - 1) as f64)) as i8),
            Col::from_zero_based((self.x.clamp(0., (board_shape.num_cols - 1) as f64)) as i8),
        )
    }

    // Returns the closest valid board coord.
    pub fn snap(self, board_shape: BoardShape) -> Self {
        FCoord {
            x: self.x.clamp(0., board_shape.num_cols as f64),
            y: self.y.clamp(0., board_shape.num_rows as f64),
        }
    }
}

impl DisplayFCoord {
    // Position of the top-left corner of a square.
    pub fn square_pivot(coord: DisplayCoord) -> Self {
        DisplayFCoord {
            x: f64::from(coord.x),
            y: f64::from(coord.y),
        }
    }

    pub fn square_center(coord: DisplayCoord) -> Self {
        DisplayFCoord {
            x: f64::from(coord.x) + 0.5,
            y: f64::from(coord.y) + 0.5,
        }
    }

    pub fn to_square(self, board_shape: BoardShape) -> Option<DisplayCoord> {
        let x = self.x as i32;
        let y = self.y as i32;
        if 0 <= x && x < board_shape.num_cols as i32 && 0 <= y && y < board_shape.num_rows as i32 {
            // Improvement potential: clamp values that are slightly out of range.
            // Who knows if all browsers guarantee click coords cannot be 0.00001px away?
            // Note: if doing this, make sure that dragging too far away doesn't highlight
            // a random edge square.
            Some(DisplayCoord {
                x: x.try_into().unwrap(),
                y: y.try_into().unwrap(),
            })
        } else {
            None
        }
    }
}

// Poor man's 2D geometry. Four vector operation should be enough for everybody.

impl ops::Add<(f64, f64)> for DisplayFCoord {
    type Output = Self;
    fn add(self, (x, y): (f64, f64)) -> Self::Output {
        DisplayFCoord { x: self.x + x, y: self.y + y }
    }
}

impl ops::Sub for DisplayFCoord {
    type Output = (f64, f64);
    fn sub(self, rhs: DisplayFCoord) -> Self::Output { (self.x - rhs.x, self.y - rhs.y) }
}

pub fn mult_vec((x, y): (f64, f64), s: f64) -> (f64, f64) { (x * s, y * s) }

pub fn normalize_vec((x, y): (f64, f64)) -> (f64, f64) { mult_vec((x, y), 1. / x.hypot(y)) }
