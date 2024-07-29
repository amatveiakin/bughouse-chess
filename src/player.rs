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

// Note. `High` is the default in order to prioritize new players for the next game. Note that this
// system cannot be cheated by toggle observer bit back and forth of leaving and rejoined the match,
// because `Participant` object for players who played at least one game is persistent.
#[derive(
    Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
pub enum PlayerSchedulingPriority {
    UltraLow, // only used temporarily for computations
    Low,      // played more games than others
    Normal,   // played less games than others
    #[default]
    High, // should be in the next game if possible
}

// Improvement potential. Similarly to how we replaced `games_missed` with `scheduling_priority`, it
// probably makes sense to replace `double_games_played` with `double_play_scheduling_priority`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participant {
    pub name: String,             // fixed for the entire match
    pub is_registered_user: bool, // fixed for the entire match
    pub active_faction: Faction,
    pub desired_faction: Faction,
    pub games_played: u32,
    pub double_games_played: u32,
    pub individual_score: HalfU32, // meaningful for Teaming::IndividualMode
    pub scheduling_priority: PlayerSchedulingPriority,
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
