use enum_map::Enum;
use serde::{Serialize, Deserialize};


#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum, Serialize, Deserialize)]
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
