use serde::{Serialize, Deserialize};

use crate::{ChessRules, BughouseRules};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContestCreationOptions {
    pub chess_rules: ChessRules,
    pub bughouse_rules: BughouseRules,
    pub player_name: String,
    pub rated: bool,
}
