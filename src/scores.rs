use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::player::Team;


// Victory is scored as 2:0, draw is 1:1.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum Scores {
    #[default]
    Zeros, // or make it optional? !!!
    // Not EnumMap, because it does not support serde.
    // Improvement potential: Implement Serde support for EnumMap instead.
    PerTeam(HashMap<Team, u32>), // when teaming == Teaming::FixedTeams
    PerPlayer(HashMap<String, u32>), // when teaming == Teaming::IndividualMode
}

impl Scores {
    pub fn new() -> Self { Scores::default() }
}
