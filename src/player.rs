use enum_map::Enum;
use serde::{Deserialize, Serialize};
use strum::EnumIter;

use crate::game::BughousePlayer;
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
    // Faction prepresents participant desired for the next. Faction can be changed at any time and
    // fall out of sync with `active_player`.
    pub faction: Faction,
    // If there is an active game, `active_player` is participant role there.
    // TODO: Remove circular dependency on `game.rs`.
    pub active_player: Option<BughousePlayer>,
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

impl Participant {
    pub fn active_team(&self) -> Option<Team> { self.active_player.map(|p| p.team()) }

    pub fn team_affiliation(&self) -> Option<Team> {
        let active_team = self.active_team();
        let faction_team = match self.faction {
            Faction::Fixed(team) => Some(team),
            Faction::Random => None,
            Faction::Observer => None,
        };
        active_team.or(faction_team)
    }

    // Returns whether the participant has ever played or wants to play in the future.
    // If false, the participant is exclusively an observer.
    pub fn is_ever_player(&self) -> bool {
        // Note. Faction can change in the middle of the game when a player requests to be an
        // observer (which will come into effect starting from the next game), so we need to check
        // the current game separately.
        let was_player = self.games_played > 0;
        let is_player = self.active_player.is_some();
        let wanna_be_player = self.faction.is_player();
        was_player || is_player || wanna_be_player
    }
}
