use enum_map::Enum;
use serde::{Deserialize, Serialize};
use strum::EnumIter;


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, EnumIter, Serialize, Deserialize)]
pub enum Team {
    Red,
    Blue,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, Serialize, Deserialize)]
pub enum Faction {
    // Play for this team for an entire match. Used only in FixedTeam mode.
    Fixed(Team),

    // Play for a random team.
    //   - In FixedTeams mode: Used only in lobby. Will be converted to `Fixed` when the
    //     match starts.
    //   - In Individual move: Used always. A player can still become an observer in any
    //     given game if there are more than four players.
    Random,

    // Always an observer. Never plays.
    Observer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participant {
    pub name: String,             // fixed for the entire match
    pub is_registered_user: bool, // fixed for the entire match
    pub faction: Faction,         // fixed for the entire match
    pub games_played: u32,
    pub double_games_played: u32,
    pub is_online: bool,
    pub is_ready: bool,
}

pub const ALL_FACTIONS: &[Faction] = &[
    Faction::Random,
    Faction::Fixed(Team::Red),
    Faction::Fixed(Team::Blue),
    Faction::Observer,
];


impl Team {
    pub fn opponent(self) -> Self {
        match self {
            Team::Red => Team::Blue,
            Team::Blue => Team::Red,
        }
    }
}

impl Faction {
    pub fn is_player(self) -> bool {
        match self {
            Faction::Fixed(_) => true,
            Faction::Random => true,
            Faction::Observer => false,
        }
    }
}
