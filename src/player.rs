use enum_map::Enum;
use serde::{Serialize, Deserialize};
use strum::EnumIter;


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, EnumIter, Serialize, Deserialize)]
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


// Player while in lobby. May or may not have an assigned team yet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub fixed_team: Option<Team>,
    pub is_online: bool,
    pub is_ready: bool,
}
