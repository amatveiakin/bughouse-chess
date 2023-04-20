use crate::coord::{Col, Coord, Row};
use crate::once_cell_regex;
use crate::piece::{CastleDirection, PieceKind};
use crate::util::as_single_char;


#[derive(Clone, Copy, Debug)]
pub enum AlgebraicCharset {
    Ascii,
    AuxiliaryUnicode,
    // Improvement potential: An option to have unicode pieces, e.g. "♞c6"
}

#[derive(Clone, Copy, Debug)]
pub enum AlgebraicDetails {
    ShortAlgebraic, // omit starting row/col when unambiguous, e.g. "e4"
    LongAlgebraic,  // always include starting rol/col, e.g. "e2e4"
}

#[derive(Clone, Copy, Debug)]
pub enum AlgebraicPromotionTarget {
    Upgrade(PieceKind),
    Discard,
    Steal((PieceKind, Coord)), // target piece and coord on the other board
}

#[derive(Clone, Debug)]
pub struct AlgebraicMove {
    pub piece_kind: PieceKind,
    pub from_col: Option<Col>,
    pub from_row: Option<Row>,
    pub capturing: bool,
    pub to: Coord,
    pub promote_to: Option<AlgebraicPromotionTarget>,
}

#[derive(Clone, Debug)]
pub struct AlgebraicDrop {
    pub piece_kind: PieceKind,
    pub to: Coord,
}

// Parsed algebraic notations. Conversion between `AlgebraicStructured` and string can be done
// without a board. Conversion between `AlgebraicStructured` and `Turn` requries a board.
#[derive(Clone, Debug)]
pub enum AlgebraicTurn {
    Move(AlgebraicMove),
    Drop(AlgebraicDrop),
    Castle(CastleDirection),
    PlaceDuck(Coord),
}


impl AlgebraicTurn {
    pub fn parse(notation: &str) -> Option<Self> {
        let notation = notation.trim();
        const PIECE_RE: &str = r"[A-Z]";
        let move_re = once_cell_regex!(&format!(
            r"^({piece})?([a-h])?([1-8])?([x×:])?([a-h][1-8])(?:[=/]?(?:({piece})|(\.)|({piece})([a-h][1-8])))?([+†#‡]?)$",
            piece = PIECE_RE
        ));
        let drop_re = once_cell_regex!(&format!(r"^({piece})@([a-h][1-8])$", piece = PIECE_RE));
        let a_castling_re = once_cell_regex!("^(0-0-0|O-O-O)$");
        let h_castling_re = once_cell_regex!("^(0-0|O-O)$");
        let place_duck_re = once_cell_regex!("^@([a-h][1-8])$");
        if let Some(cap) = move_re.captures(notation) {
            let piece_kind = match cap.get(1) {
                Some(m) => PieceKind::from_algebraic(m.as_str())?,
                None => PieceKind::Pawn,
            };
            let from_col = cap
                .get(2)
                .map(|m| Col::from_algebraic(as_single_char(m.as_str()).unwrap()).unwrap());
            let from_row = cap
                .get(3)
                .map(|m| Row::from_algebraic(as_single_char(m.as_str()).unwrap()).unwrap());
            let capturing = cap.get(4).is_some();
            let to = Coord::from_algebraic(cap.get(5).unwrap().as_str()).unwrap();
            let upgdate_promotion = match cap.get(6) {
                Some(m) => Some(PieceKind::from_algebraic(m.as_str())?),
                None => None,
            };
            let discard_promotion = cap.get(7).map(|_| ());
            let steal_promotion = match (cap.get(8), cap.get(9)) {
                (Some(m1), Some(m2)) => Some((
                    PieceKind::from_algebraic(m1.as_str())?,
                    Coord::from_algebraic(m2.as_str()).unwrap(),
                )),
                _ => None,
            };
            let promote_to = match (upgdate_promotion, discard_promotion, steal_promotion) {
                (Some(piece_kind), None, None) => {
                    Some(AlgebraicPromotionTarget::Upgrade(piece_kind))
                }
                (None, Some(_), None) => Some(AlgebraicPromotionTarget::Discard),
                (None, None, Some((piece_kind, pos))) => {
                    Some(AlgebraicPromotionTarget::Steal((piece_kind, pos)))
                }
                (None, None, None) => None,
                _ => panic!("Multiple promotion rules detected."),
            };
            let _mark = cap.get(10).map(|m| m.as_str()); // TODO: Test check/mate

            Some(AlgebraicTurn::Move(AlgebraicMove {
                piece_kind,
                from_row,
                from_col,
                capturing,
                to,
                promote_to,
            }))
        } else if let Some(cap) = drop_re.captures(notation) {
            let piece_kind = PieceKind::from_algebraic(cap.get(1).unwrap().as_str())?;
            let to = Coord::from_algebraic(cap.get(2).unwrap().as_str()).unwrap();
            Some(AlgebraicTurn::Drop(AlgebraicDrop { piece_kind, to }))
        } else if a_castling_re.is_match(notation) {
            Some(AlgebraicTurn::Castle(CastleDirection::ASide))
        } else if h_castling_re.is_match(notation) {
            Some(AlgebraicTurn::Castle(CastleDirection::HSide))
        } else if let Some(cap) = place_duck_re.captures(notation) {
            let to = Coord::from_algebraic(cap.get(1).unwrap().as_str()).unwrap();
            Some(AlgebraicTurn::PlaceDuck(to))
        } else {
            None
        }
    }

    pub fn format(&self, charset: AlgebraicCharset) -> String {
        match self {
            AlgebraicTurn::Move(mv) => {
                let capture_notation = match charset {
                    AlgebraicCharset::Ascii => "x",
                    AlgebraicCharset::AuxiliaryUnicode => "×",
                };
                let promotion_sep = match charset {
                    AlgebraicCharset::Ascii => "=",            // pgn convention
                    AlgebraicCharset::AuxiliaryUnicode => "/", // takes less space in log
                };
                let mut from = String::new();
                if let Some(col) = mv.from_col {
                    from.push(col.to_algebraic())
                };
                if let Some(row) = mv.from_row {
                    from.push(row.to_algebraic())
                };
                let promotion = match mv.promote_to {
                    Some(AlgebraicPromotionTarget::Upgrade(piece_kind)) => {
                        format!("{}{}", promotion_sep, piece_kind.to_full_algebraic())
                    }
                    Some(AlgebraicPromotionTarget::Discard) => format!("{}.", promotion_sep),
                    Some(AlgebraicPromotionTarget::Steal((piece_kind, pos))) => {
                        format!(
                            "{}{}{}",
                            promotion_sep,
                            piece_kind.to_full_algebraic(),
                            pos.to_algebraic()
                        )
                    }
                    None => String::new(),
                };
                format!(
                    "{}{}{}{}{}",
                    mv.piece_kind.to_algebraic_for_move(),
                    from,
                    if mv.capturing { capture_notation } else { "" },
                    mv.to.to_algebraic(),
                    promotion,
                )
            }
            AlgebraicTurn::Drop(drop) => {
                format!("{}@{}", drop.piece_kind.to_full_algebraic(), drop.to.to_algebraic())
            }
            AlgebraicTurn::Castle(dir) => match dir {
                CastleDirection::ASide => "O-O-O".to_owned(),
                CastleDirection::HSide => "O-O".to_owned(),
            },
            AlgebraicTurn::PlaceDuck(to) => format!("@{}", to.to_algebraic()),
        }
    }

    pub fn format_in_the_fog(&self) -> String {
        match self {
            AlgebraicTurn::Move(..) | AlgebraicTurn::Castle(..) => "🌫".to_owned(),
            AlgebraicTurn::Drop(drop) => format!("{}@🌫", drop.piece_kind.to_full_algebraic()),
            AlgebraicTurn::PlaceDuck(..) => self.format(AlgebraicCharset::AuxiliaryUnicode),
        }
    }
}
