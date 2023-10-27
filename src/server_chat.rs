use std::collections::{HashSet, VecDeque};

use crate::chat::{ChatRecipient, MAX_CHAT_MESSAGES, MAX_CHAT_MESSAGE_LENGTH};
use crate::game::GameOutcome;
use crate::player::Team;
use crate::utc_time::UtcDateTime;


// The information in `ChatRecipientExpanded` should be sufficient to determine the list of
// recipients not just when the message is sent, but also later when new users join. Therefore:
//   - Public messages stay public, so `All` is expanded to `All`.
//   - Direct messages stay direct, so `Participant(x)` is expanded to `Participants(vec![x])`.
//   - Team message are more complicated. In fixed teams mode `Team` is expanded to `FixedTeam`
//     which means it is delivered to all team members, including newly joined ones (this is not
//     supported at the time of writing, but should be supported in the future). In dynamic teams
//     mode the message is delivered to whoever is in the team at the moment of sending, so `Team`
//     is expanded to `Participants`.
#[derive(Clone, Debug)]
pub enum ChatRecipientExpanded {
    All,
    FixedTeam(Team),
    Participants(HashSet<String>),
}

#[derive(Clone, Debug)]
pub enum ChatMessageBodyExpanded {
    Regular {
        sender: String,
        recipient: ChatRecipient,
        recipient_expanded: ChatRecipientExpanded,
        text: String,
    },
    GameOver {
        outcome: GameOutcome,
    },
}

#[derive(Clone, Debug)]
pub struct ChatMessageExpanded {
    pub message_id: u64,
    pub game_index: Option<u64>,
    pub time: UtcDateTime,
    pub body: ChatMessageBodyExpanded,
}

#[derive(Clone, Debug)]
pub struct ServerChat {
    messages: VecDeque<ChatMessageExpanded>,
    first_new_message_id: u64,
    next_id: u64,
}

impl ServerChat {
    pub fn new() -> Self {
        ServerChat {
            messages: VecDeque::new(),
            first_new_message_id: 0,
            next_id: 0,
        }
    }

    pub fn first_new_message_id(&self) -> u64 { self.first_new_message_id }
    pub fn reset_first_new_message_id(&mut self) { self.first_new_message_id = self.next_id; }

    pub fn messages_since(&self, start: usize) -> impl Iterator<Item = &ChatMessageExpanded> {
        self.messages.range(start..)
    }
    pub fn all_messages(&self) -> impl Iterator<Item = &ChatMessageExpanded> {
        self.messages.iter()
    }

    pub fn add(
        &mut self, game_index: Option<u64>, time: UtcDateTime, mut body: ChatMessageBodyExpanded,
    ) {
        // TODO: Check message length on the client and don't allow to send longer messages instead.
        // Also add a small "N characted left" widget when close to the limit (like SMS apps or
        // comments on StackOverflow do).
        match body {
            ChatMessageBodyExpanded::Regular { ref mut text, .. } => {
                // Improvement potential. Apply NFC and count Unicode graphemes instead. Need to
                // check how this affects WASM size, though.
                *text = text.chars().take(MAX_CHAT_MESSAGE_LENGTH).collect()
            }
            ChatMessageBodyExpanded::GameOver { .. } => {}
        }
        let message_id = self.next_id;
        self.next_id += 1;
        let message = ChatMessageExpanded { message_id, game_index, time, body };
        self.messages.push_back(message);
        while self.messages.len() > MAX_CHAT_MESSAGES {
            self.messages.pop_front();
        }
    }
}

// Returns messages added since the last call to `fetch_new_messages`.
//
// Rust-upgrade: Change this to a member function like this
//   pub fn fetch_new_messages(&mut self) -> impl Iterator<Item = &ChatMessageExpanded> { ... }
// when Rust allows to express the fact that we only an immutable borrow to `self `exists after the
// function returns. See
// https://users.rust-lang.org/t/return-immutable-reference-taking-mutable-reference-to-self/16970
#[macro_export]
macro_rules! fetch_new_chat_messages {
    ($chat:expr) => {{
        let start = $chat.first_new_message_id() as usize;
        $chat.reset_first_new_message_id();
        $chat.messages_since(start)
    }};
}
