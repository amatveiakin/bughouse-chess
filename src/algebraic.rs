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

#[derive(Clone, Debug)]
pub struct AlgebraicMove {
    pub piece_kind: PieceKind,
    pub from_col: Option<Col>,
    pub from_row: Option<Row>,
    pub capturing: bool,
    pub to: Coord,
    pub promote_to: Option<PieceKind>,
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
            r"^({piece})?([a-h])?([1-8])?([x×:])?([a-h][1-8])(?:[=/]?({piece})?)([+†#‡]?)$",
            piece = PIECE_RE
        ));
        let drop_re = once_cell_regex!(&format!(r"^({piece})@([a-h][1-8])$", piece = PIECE_RE));
        let a_castling_re = once_cell_regex!("^(0-0-0|O-O-O)$");
        let h_castling_re = once_cell_regex!("^(0-0|O-O)$");
        let place_duck_re = once_cell_regex!("^@([a-h][1-8])$");
        if let Some(cap) = move_re.captures(notation) {
            let piece_kind = match cap.get(1) {
                None => PieceKind::Pawn,
                Some(m) => PieceKind::from_algebraic(m.as_str())?,
            };
            let from_col = cap
                .get(2)
                .map(|m| Col::from_algebraic(as_single_char(m.as_str()).unwrap()).unwrap());
            let from_row = cap
                .get(3)
                .map(|m| Row::from_algebraic(as_single_char(m.as_str()).unwrap()).unwrap());
            let capturing = cap.get(4).is_some();
            let to = Coord::from_algebraic(cap.get(5).unwrap().as_str()).unwrap();
            let promote_to = match cap.get(6) {
                None => None,
                Some(m) => Some(PieceKind::from_algebraic(m.as_str())?),
            };
            let _mark = cap.get(7).map(|m| m.as_str()); // TODO: Test check/mate
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
                let mut from = String::new();
                if let Some(col) = mv.from_col {
                    from.push(col.to_algebraic())
                };
                if let Some(row) = mv.from_row {
                    from.push(row.to_algebraic())
                };
                let promotion = match mv.promote_to {
                    Some(piece_kind) => format!("={}", piece_kind.to_full_algebraic()),
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
