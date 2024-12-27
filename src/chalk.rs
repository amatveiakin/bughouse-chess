use std::collections::HashMap;

use enum_map::EnumMap;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::coord::{BoardShape, Coord};
use crate::display::{
    display_to_fcoord, get_board_orientation, DisplayBoard, DisplayFCoord, FCoord, Perspective,
};
use crate::game::BughouseBoard;
use crate::piece::PieceKind;


#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum ChalkMark {
    Arrow { from: Coord, to: Coord },
    // Improvement potential: Smoothen and simplify the curve. Either while drawing
    //   or afterwards. Or both.
    FreehandLine { points: Vec<FCoord> },
    SquareHighlight { coord: Coord },
    GhostPiece { coord: Coord, piece_kind: PieceKind },
}

// Represents all chalk marks by a given player.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChalkDrawing {
    pub board: EnumMap<BughouseBoard, Vec<ChalkMark>>,
}

impl ChalkDrawing {
    pub fn new() -> Self { ChalkDrawing::default() }
    pub fn board(&self, board_idx: BughouseBoard) -> &Vec<ChalkMark> { &self.board[board_idx] }
    pub fn board_mut(&mut self, board_idx: BughouseBoard) -> &mut Vec<ChalkMark> {
        &mut self.board[board_idx]
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Chalkboard {
    player_drawings: HashMap<String, ChalkDrawing>,
}

impl Chalkboard {
    pub fn new() -> Self { Chalkboard::default() }

    pub fn all_drawings(&self) -> &HashMap<String, ChalkDrawing> { &self.player_drawings }
    pub fn drawings_by(&self, player_name: &str) -> Option<&ChalkDrawing> {
        self.player_drawings.get(player_name)
    }

    pub fn add_mark(&mut self, player: String, board_idx: BughouseBoard, mark: ChalkMark) {
        let marks = self.player_drawings.entry(player).or_default().board_mut(board_idx);
        if let Some(existing) = marks.iter().position(|m| *m == mark) {
            marks.remove(existing);
        } else {
            marks.push(mark);
        }
    }
    pub fn remove_last_mark(&mut self, player: String, board_idx: BughouseBoard) {
        self.player_drawings.entry(player).or_default().board_mut(board_idx).pop();
    }
    pub fn clear_drawing(&mut self, player: String, board_idx: BughouseBoard) -> bool {
        let board = &mut self.player_drawings.entry(player).or_default().board_mut(board_idx);
        let had_content = !board.is_empty();
        board.clear();
        had_content
    }
    pub fn clear_drawings_by_player(&mut self, player: String) -> bool {
        let mut had_content = false;
        for board_idx in BughouseBoard::iter() {
            had_content |= self.clear_drawing(player.clone(), board_idx);
        }
        had_content
    }
    pub fn set_drawing(&mut self, player: String, drawing: ChalkDrawing) {
        self.player_drawings.insert(player, drawing);
    }
}

#[derive(Debug)]
pub struct ChalkCanvas {
    board_shape: BoardShape,
    perspective: Perspective,
    painting: Option<(DisplayBoard, ChalkMark)>,
}

impl ChalkCanvas {
    pub fn new(board_shape: BoardShape, perspective: Perspective) -> Self {
        ChalkCanvas { board_shape, perspective, painting: None }
    }

    pub fn is_painting(&self) -> bool { self.painting.is_some() }
    pub fn current_painting(&self) -> Option<&(DisplayBoard, ChalkMark)> {
        self.painting.as_ref().filter(|&p| is_valid_painting(p))
    }

    pub fn chalk_down(
        &mut self, board_idx: DisplayBoard, pos: DisplayFCoord, alternative_mode: bool,
    ) {
        let fcoord = to_fcoord(self.board_shape, self.perspective, board_idx, pos);
        if alternative_mode {
            self.painting = Some((board_idx, ChalkMark::FreehandLine { points: vec![fcoord] }));
        } else {
            let coord = fcoord.to_coord_snapped(self.board_shape);
            self.painting = Some((board_idx, ChalkMark::Arrow { from: coord, to: coord }));
        }
    }

    pub fn chalk_move(&mut self, pos: DisplayFCoord) {
        let Some((board_idx, ref mut mark)) = self.painting else {
            return;
        };
        let fcoord = to_fcoord(self.board_shape, self.perspective, board_idx, pos);
        match mark {
            &mut ChalkMark::Arrow { ref mut to, .. } => {
                *to = fcoord.to_coord_snapped(self.board_shape);
            }
            &mut ChalkMark::FreehandLine { ref mut points } => {
                let fcoord = fcoord.snap(self.board_shape);
                // Possible optimization: also filter out consequent points that are very close.
                if points.last() != Some(&fcoord) {
                    points.push(fcoord);
                }
            }
            ChalkMark::SquareHighlight { .. } => {}
            ChalkMark::GhostPiece { .. } => {}
        }
    }

    #[must_use]
    pub fn chalk_up(&mut self, pos: DisplayFCoord) -> Option<(DisplayBoard, ChalkMark)> {
        self.chalk_move(pos);
        let painting = self.painting.take();
        if let Some((board_idx, ChalkMark::Arrow { to, from })) = &painting {
            if to == from {
                return Some((*board_idx, ChalkMark::SquareHighlight { coord: *to }));
            }
        }
        painting.filter(is_valid_painting)
    }

    pub fn chalk_abort(&mut self) { self.painting = None; }
}

fn to_fcoord(
    board_shape: BoardShape, perspective: Perspective, board_idx: DisplayBoard, pos: DisplayFCoord,
) -> FCoord {
    let orientation = get_board_orientation(board_idx, perspective);
    display_to_fcoord(pos, board_shape, orientation)
}

fn is_valid_painting((_, mark): &(DisplayBoard, ChalkMark)) -> bool {
    match mark {
        ChalkMark::Arrow { to, from } => to != from,
        ChalkMark::FreehandLine { points } => points.len() > 1,
        ChalkMark::SquareHighlight { .. } => true,
        ChalkMark::GhostPiece { .. } => true,
    }
}
