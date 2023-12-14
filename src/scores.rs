use std::collections::HashMap;

use enum_map::EnumMap;
use serde::{Deserialize, Serialize};

use crate::half_integer::HalfU32;
use crate::player::Team;


// Victory is scored as 1 : 0, draw is 1/2 : 1/2.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Scores {
    PerTeam(EnumMap<Team, HalfU32>), // when teaming == Teaming::FixedTeams
    PerPlayer(HashMap<String, HalfU32>), // when teaming == Teaming::IndividualMode
}
