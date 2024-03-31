use serde::{Deserialize, Serialize};

use crate::game::{GameOutcome, PlayerInGame};
use crate::lobby::ParticipantsError;
use crate::player::Faction;
use crate::utc_time::UtcDateTime;


pub const MAX_CHAT_MESSAGES: usize = 1000;
pub const MAX_CHAT_MESSAGE_LENGTH: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChatRecipient {
    All,
    Team,
    Participant(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChatMessageBody {
    Regular {
        sender: String,
        recipient: ChatRecipient,
        text: String,
    },
    FactionChanged {
        participant: String,
        old_faction: Faction,
        new_faction: Faction,
    },
    GameOver {
        outcome: GameOutcome,
    },
    NextGamePlayers {
        players: Vec<PlayerInGame>,
    },
    CannotStartGame {
        error: ParticipantsError,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub message_id: u64,
    // `game_index` == `None` means sent before the first game started. This is not possible at the
    // time of writing, but there nothing wrong with it in principle.
    pub game_index: Option<u64>,
    pub time: UtcDateTime,
    pub body: ChatMessageBody,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutgoingChatMessage {
    pub local_message_id: u64,
    pub recipient: ChatRecipient,
    pub text: String,
}
