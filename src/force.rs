use enum_map::Enum;
use serde::{Deserialize, Serialize};
use strum::EnumIter;


#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Enum, EnumIter, Serialize, Deserialize,
)]
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
