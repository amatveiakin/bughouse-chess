use enum_map::Enum;
use serde::{Deserialize, Serialize};
use strum::EnumIter;

use crate::half_integer::HalfU32;


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, EnumIter, Serialize, Deserialize)]
pub enum Team {
    Red,
    Blue,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, Serialize, Deserialize)]
pub enum Faction {
    // Always play for this team.
    //   - With FixedTeams: this is your team.
    //   - With IndividualMode: it is still possible to have a fixed team. In this case you never
    //     play against people with the same fixed team; and you never play together with people
    //     with another fixed team.
    // May seat out and observe sometimes if there are too many players.
    Fixed(Team),

    // Play for a random team. Possible only in IndividualMode.
    // May seat out and observe sometimes if there are too many players.
    Random,

    // Always an observer. Never plays.
    Observer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participant {
    pub name: String,             // fixed for the entire match
    pub is_registered_user: bool, // fixed for the entire match
    pub active_faction: Faction,
    pub desired_faction: Faction,
    pub games_played: u32,
    pub games_missed: u32, // was ready to play, but had to seat out
    pub double_games_played: u32,
    pub individual_score: HalfU32, // meaningful for Teaming::IndividualMode
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
