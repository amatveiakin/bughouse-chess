use std::collections::{HashSet, VecDeque};

use crate::chat::{ChatMessage, ChatMessageBody, MAX_CHAT_MESSAGE_LENGTH, MAX_CHAT_MESSAGES};
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
pub struct ServerChat {
    messages: VecDeque<(ChatRecipientExpanded, ChatMessage)>,
    last_sent_message_id: Option<u64>, // ID of last message broadcast to all clients
    next_id: u64,
}

impl ServerChat {
    pub fn new() -> Self {
        ServerChat {
            messages: VecDeque::new(),
            last_sent_message_id: None,
            next_id: 0,
        }
    }

    pub fn all_messages(&self) -> impl Iterator<Item = &(ChatRecipientExpanded, ChatMessage)> {
        self.messages.iter()
    }
    pub fn fetch_new_messages(&mut self) -> Vec<(ChatRecipientExpanded, ChatMessage)> {
        let last_send_id = self.last_sent_message_id;
        self.last_sent_message_id = self.messages.back().map(|(_, m)| m.message_id);
        let Some(start_id) = last_send_id else {
            // Sending messages to clients for the first time: everything is new.
            return self.messages.iter().cloned().collect();
        };
        let last_sent_index = self.messages.iter().position(|(_, m)| m.message_id == start_id);
        if let Some(last_sent_index) = last_sent_index {
            // We've already sent message up to and including `last_sent_index`. Send everything
            // after that.
            self.messages.range((last_sent_index + 1)..).cloned().collect()
        } else {
            // We got so many new messages at once that all of the messages we saw last time were
            // already discarded.
            self.messages.iter().cloned().collect()
        }
    }

    pub fn add(
        &mut self, game_index: Option<u64>, time: UtcDateTime,
        recipient_expanded: ChatRecipientExpanded, mut body: ChatMessageBody,
    ) {
        // TODO: Check message length on the client and don't allow to send longer messages instead.
        // Also add a small "N characted left" widget when close to the limit (like SMS apps or
        // comments on StackOverflow do).
        match body {
            ChatMessageBody::Regular { ref mut text, .. } => {
                // Improvement potential. Apply NFC and count Unicode graphemes instead. Need to
                // check how this affects WASM size, though.
                *text = text.chars().take(MAX_CHAT_MESSAGE_LENGTH).collect()
            }
            ChatMessageBody::FactionChanged { .. } => {}
            ChatMessageBody::GameOver { .. } => {}
            ChatMessageBody::NextGamePlayers { .. } => {}
            ChatMessageBody::CannotStartGame { .. } => {}
        }
        let message_id = self.next_id;
        self.next_id += 1;
        let message = ChatMessage { message_id, game_index, time, body };
        self.messages.push_back((recipient_expanded, message));
        while self.messages.len() > MAX_CHAT_MESSAGES {
            self.messages.pop_front();
        }
    }
}
