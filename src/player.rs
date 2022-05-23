use enum_map::Enum;
use serde::{Serialize, Deserialize};


#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum, Serialize, Deserialize)]
pub enum Team {
    Red,
    Blue,
}

impl Team {
    pub fn opponent(self) -> Self {
        match self {
            Team::Red => Team::Blue,
            Team::Blue => Team::Red,
        }
    }
}


// Improvement potential: Find a consistent and efficient way to store and address players.
//   Identify by reference / id / visible name / internal name ?..
//   Or may be use (BughouseBoard, Force) pair as an ID?
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub team: Team,
}
