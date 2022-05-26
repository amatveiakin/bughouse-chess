use enum_map::Enum;
use serde::{Serialize, Deserialize};
use strum::EnumIter;


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, EnumIter, Serialize, Deserialize)]
pub enum Force {
    White,
    Black,
}

impl Force {
    pub fn opponent(self) -> Force {
        match self {
            Force::White => Force::Black,
            Force::Black => Force::White,
        }
    }
}
