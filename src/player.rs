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


// TODO: How to store players? Identify by reference / id / visible name / internal name ?..
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub team: Team,
}
