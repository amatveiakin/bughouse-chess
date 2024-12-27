use std::collections::VecDeque;

use enum_map::enum_map;

use crate::chat::{
    ChatMessage, ChatMessageBody, ChatRecipient, MAX_CHAT_MESSAGES, OutgoingChatMessage,
};
use crate::lobby::ParticipantsError;
use crate::player::{Faction, Team};
use crate::rules::ChessRules;


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SystemMessageClass {
    Info,
    Error,
    GameOver,
}

// Note. There is no "Team" party, because this struct is responsible for the visual representation
// of the chat, and team is the default, so we don't show any recipient in this case.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ChatParty {
    Myself,                     // sender or recipient
    Participant(String),        // sender or recipient
    All,                        // always recipient
    System(SystemMessageClass), // always sender
}

#[derive(Clone, Debug)]
pub struct EphemeralSystemMessage {
    ephemeral_message_id: u64,
    class: SystemMessageClass,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ChatItemDurability {
    Static,    // message from the server (could be sent by us, another player or system)
    Local,     // message not yet confirmed by the server
    Ephemeral, // message will be removed when another message is added; never sent to the server
}

#[derive(Clone, Debug)]
pub struct ChatItem {
    pub id: String,
    pub durability: ChatItemDurability,
    pub text: String,
    pub sender: Option<ChatParty>,
    pub recipient: Option<ChatParty>,
    pub dimmed: bool,    // message will be less noticeable
    pub prominent: bool, // message will be bolder and more prominent
    pub flash: bool,     // message will be highlighted upon addition
}

#[derive(Clone, Debug)]
pub struct ClientChat {
    static_messages: VecDeque<ChatMessage>,
    local_messages: Vec<OutgoingChatMessage>,
    ephemeral_message: Option<EphemeralSystemMessage>,
    next_local_id: u64,
    next_ephemeral_id: u64,
}

impl ClientChat {
    pub fn new() -> Self {
        Self {
            static_messages: VecDeque::new(),
            local_messages: Vec::new(),
            ephemeral_message: None,
            next_local_id: 0,
            next_ephemeral_id: 0,
        }
    }

    pub fn local_messages(&self) -> impl ExactSizeIterator<Item = &OutgoingChatMessage> {
        self.local_messages.iter()
    }

    pub fn items(
        &self, my_name: &str, chess_rules: &ChessRules, game_index: Option<u64>,
    ) -> Vec<ChatItem> {
        (self
            .static_messages
            .iter()
            .filter_map(|m| static_message_to_item(m, my_name, chess_rules, game_index)))
        .chain(self.local_messages.iter().map(local_message_to_item))
        .chain(self.ephemeral_message.iter().map(ephemeral_message_to_item))
        .collect()
    }

    pub fn add_static(&mut self, message: ChatMessage) {
        if let Some(latest) = self.static_messages.back() {
            if message.message_id <= latest.message_id {
                return;
            }
        }
        self.remove_ephemeral();
        self.static_messages.push_back(message);
        while self.static_messages.len() > MAX_CHAT_MESSAGES {
            self.static_messages.pop_front();
        }
    }
    pub fn add_local(&mut self, recipient: ChatRecipient, text: String) -> &OutgoingChatMessage {
        self.remove_ephemeral();
        let message = OutgoingChatMessage {
            local_message_id: self.next_local_id,
            recipient,
            text,
        };
        self.next_local_id += 1;
        self.local_messages.push(message);
        self.local_messages.last().unwrap()
    }
    pub fn add_ephemeral_system_message(&mut self, class: SystemMessageClass, text: String) {
        self.remove_ephemeral();
        let message = EphemeralSystemMessage {
            ephemeral_message_id: self.next_ephemeral_id,
            class,
            text,
        };
        self.next_ephemeral_id += 1;
        self.ephemeral_message = Some(message);
    }

    pub fn remove_confirmed_local(&mut self, confirmed_local_message_id: u64) {
        self.local_messages.retain(|m| m.local_message_id > confirmed_local_message_id)
    }
    pub fn remove_ephemeral(&mut self) { self.ephemeral_message = None; }
}

fn chat_item_id(prefix: &str, sub_id: u64) -> String { format!("{prefix}-{sub_id:08}") }

fn static_message_to_item(
    message: &ChatMessage, my_name: &str, chess_rules: &ChessRules, game_index: Option<u64>,
) -> Option<ChatItem> {
    let id = chat_item_id("a", message.message_id);
    let old_game = message.game_index != game_index;
    match &message.body {
        ChatMessageBody::Regular { sender, recipient, text } => {
            let sender_party = if sender == my_name {
                ChatParty::Myself
            } else {
                ChatParty::Participant(sender.clone())
            };
            let recipient_party = match recipient {
                ChatRecipient::All => Some(ChatParty::All),
                ChatRecipient::Team => None,
                ChatRecipient::Participant(name) if name == my_name => Some(ChatParty::Myself),
                ChatRecipient::Participant(name) => Some(ChatParty::Participant(name.clone())),
            };
            Some(ChatItem {
                id,
                durability: ChatItemDurability::Static,
                text: text.clone(),
                sender: Some(sender_party),
                recipient: recipient_party,
                dimmed: old_game,
                prominent: false,
                flash: false,
            })
        }
        ChatMessageBody::FactionChanged { participant, new_faction, .. } => {
            // Note. The messages are based on the fact that we allow switching to observer and
            // back. If other faction changes are allowed, these probably need to be updated.
            let text = match new_faction {
                Faction::Fixed(Team::Red) => format!("{participant} joined team Red"),
                Faction::Fixed(Team::Blue) => format!("{participant} joined team Blue"),
                Faction::Random => format!("{participant} is going to play"),
                Faction::Observer => format!("{participant} became an observer"),
            };
            Some(ChatItem {
                id,
                durability: ChatItemDurability::Static,
                text,
                sender: Some(ChatParty::System(SystemMessageClass::Info)),
                recipient: None,
                dimmed: old_game,
                prominent: false,
                flash: false,
            })
        }
        ChatMessageBody::GameOver { outcome } => {
            let outcome = outcome.to_readable_string(chess_rules);
            let highlight = !old_game;
            Some(ChatItem {
                id,
                durability: ChatItemDurability::Static,
                text: format!("Game over! {outcome}."),
                sender: Some(ChatParty::System(SystemMessageClass::GameOver)),
                recipient: None,
                dimmed: old_game,
                prominent: highlight,
                flash: highlight,
            })
        }
        ChatMessageBody::NextGamePlayers { players } => {
            if old_game {
                // Improvement potential. Consider if this is confusing. Can we reduce chat clutter
                // without editing the content retroactively?
                return None;
            }
            let mut teams = enum_map! {_ => vec![]};
            for p in players {
                teams[p.id.team()].push(p.name.clone());
            }
            Some(ChatItem {
                id,
                durability: ChatItemDurability::Static,
                text: format!(
                    "Next up: {} vs {}.",
                    teams[Team::Red].join(" & "),
                    teams[Team::Blue].join(" & ")
                ),
                sender: Some(ChatParty::System(SystemMessageClass::Info)),
                recipient: None,
                dimmed: old_game,
                prominent: false,
                flash: false,
            })
        }
        ChatMessageBody::CannotStartGame { error } => Some(ChatItem {
            id,
            durability: ChatItemDurability::Static,
            text: cannot_start_game_message(*error).to_owned(),
            sender: Some(ChatParty::System(SystemMessageClass::Error)),
            recipient: None,
            dimmed: old_game,
            prominent: false,
            flash: false,
        }),
    }
}


pub fn cannot_start_game_message(error: ParticipantsError) -> &'static str {
    match error {
        ParticipantsError::NotEnoughPlayers => "Not enough players",
        ParticipantsError::EmptyTeam => "A team is empty",
        ParticipantsError::RatedDoublePlay => "Cannot play on two boards in rated",
    }
}

fn local_message_to_item(message: &OutgoingChatMessage) -> ChatItem {
    let id = chat_item_id("b", message.local_message_id);
    let recipient_party = match &message.recipient {
        ChatRecipient::All => Some(ChatParty::All),
        ChatRecipient::Team => None,
        ChatRecipient::Participant(name) => Some(ChatParty::Participant(name.clone())),
    };
    ChatItem {
        id,
        durability: ChatItemDurability::Local,
        text: message.text.clone(),
        sender: Some(ChatParty::Myself),
        recipient: recipient_party,
        dimmed: false,
        prominent: false,
        flash: false,
    }
}

fn ephemeral_message_to_item(message: &EphemeralSystemMessage) -> ChatItem {
    let id = chat_item_id("c", message.ephemeral_message_id);
    let flash = match message.class {
        SystemMessageClass::Info => false,
        SystemMessageClass::Error => true,
        SystemMessageClass::GameOver => true, // shouldn't happen: game over message must be static
    };
    ChatItem {
        id,
        durability: ChatItemDurability::Ephemeral,
        text: message.text.clone(),
        sender: Some(ChatParty::System(message.class)),
        recipient: None,
        dimmed: false,
        prominent: false,
        flash,
    }
}
