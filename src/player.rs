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
}

// Player in an active game. Always has a team.
// TODO: Is the team required here at all?
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerInGame {
    pub name: String,
    pub team: Team,
}
