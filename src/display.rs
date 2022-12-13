use strum::EnumIter;

use crate::coord::{Row, Col, Coord, NUM_ROWS, NUM_COLS};
use crate::force::Force;
use crate::game::BughouseBoard;


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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Perspective {
    PlayAsWhite,
    PlayAsBlack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoardOrientation {
    Normal,
    Rotated,
}

// These coords describe board squares, like `Coord`. Both `x` and `y` are integers
// between 0 and 7. But here row 0 corresponds to the top-most row, which could be
// row '1' or row '8' on the board.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayCoord {
    pub x: u8,
    pub y: u8,
}


impl Perspective {
    pub fn for_force(force: Force) -> Self {
        match force {
            Force::White => Perspective::PlayAsWhite,
            Force::Black => Perspective::PlayAsBlack,
        }
    }
}

pub fn get_board_index(board: DisplayBoard, viewer: BughouseParticipantId) -> BughouseBoard {
    match board {
        DisplayBoard::Primary => viewer.visual_board_idx(),
        DisplayBoard::Secondary => viewer.visual_board_idx().other(),
    }
}

pub fn get_board_orientation(board: DisplayBoard, perspective: Perspective) -> BoardOrientation {
    use DisplayBoard::*;
    use Perspective::*;
    match (board, perspective) {
        (Primary, PlayAsWhite) | (Secondary, PlayAsBlack) => BoardOrientation::Normal,
        (Primary, PlayAsBlack) | (Secondary, PlayAsWhite) => BoardOrientation::Rotated,
    }
}

pub fn to_display_coord(coord: Coord, orientation: BoardOrientation) -> DisplayCoord {
    match orientation {
        BoardOrientation::Normal => DisplayCoord {
            x: coord.col.to_zero_based(),
            y: NUM_ROWS - coord.row.to_zero_based() - 1,
        },
        BoardOrientation::Rotated => DisplayCoord {
            x: NUM_COLS - coord.col.to_zero_based() - 1,
            y: coord.row.to_zero_based(),
        },
    }
}

pub fn from_display_row(y: u8, orientation: BoardOrientation) -> Row {
    match orientation {
        BoardOrientation::Normal => Row::from_zero_based(NUM_ROWS - y - 1),
        BoardOrientation::Rotated => Row::from_zero_based(y),
    }
}

pub fn from_display_col(x: u8, orientation: BoardOrientation) -> Col {
    match orientation {
        BoardOrientation::Normal => Col::from_zero_based(x),
        BoardOrientation::Rotated => Col::from_zero_based(NUM_COLS - x - 1),
    }
}

pub fn from_display_coord(coord: DisplayCoord, orientation: BoardOrientation) -> Coord {
    Coord {
        row: from_display_row(coord.y, orientation),
        col: from_display_col(coord.x, orientation),
    }
}

// Position of the top-left corner of a square.
pub fn square_position(coord: DisplayCoord) -> (f64, f64) {
    return (
        f64::from(coord.x),
        f64::from(coord.y),
    );
}

pub fn position_to_square(x: f64, y: f64) -> Option<DisplayCoord> {
    let x = x as i32;
    let y = y as i32;
    if 0 <= x && x < NUM_COLS as i32 && 0 <= y && y < NUM_ROWS as i32 {
        // Improvement potential: clamp instead of asserting the values are in range.
        // Who knows if all browsers guarantee click coords cannot be 0.00001px away?
        Some(DisplayCoord{ x: x.try_into().unwrap(), y: y.try_into().unwrap() })
    } else {
        None
    }
}
