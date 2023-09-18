use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::player::Team;


// Victory is scored as 2:0, draw is 1:1.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Scores {
    // Not EnumMap, because it does not support serde.
    // Improvement potential: Implement Serde support for EnumMap instead.
    PerTeam(HashMap<Team, u32>), // when teaming == Teaming::FixedTeams
    PerPlayer(HashMap<String, u32>), // when teaming == Teaming::IndividualMode
}
