use std::collections::HashMap;

use enum_map::EnumMap;
use serde::{Deserialize, Serialize};

use crate::player::Team;


// Victory is scored as 2:0, draw is 1:1.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Scores {
    PerTeam(EnumMap<Team, u32>),     // when teaming == Teaming::FixedTeams
    PerPlayer(HashMap<String, u32>), // when teaming == Teaming::IndividualMode
}
