// TODO: Show message time. Option questions:
//   - Should it be on the left (before the sender name) or in the bottom right corner like in most
//     chat app? The latter would probably require separating messages visually with some kind of
//     bubbles.
//   - Should we show absolute time or relative time (e.g. "5m ago")? The latter would creating
//     visual noise from updating times. The former may not be very useful without introducing
//     clocks into the app if there are a lot of people who play in full screen (like I do).
//
// Improvement potential. Make simple updates, like adding a new message, O(1).

use bughouse_chess::client_chat::{ChatItem, ChatItemDurability, ChatParty, SystemMessageClass};
use bughouse_chess::game::{BughouseParticipant, BughousePlayer};

use crate::rust_error;
use crate::web_document::web_document;
use crate::web_element_ext::WebElementExt;
use crate::web_error_handling::JsResult;
use crate::web_util::{remove_all_children, scroll_to_bottom};


const CHAT_ID_ATTR: &str = "data-chat-item-id";

// Assumptions:
//   - chat items can be added at any position, not just at the end;
//   - chat items can be removed at any position;
//   - chat items are never reordered;
//   - chat item id is never changed.
//
// If a node for a given item already exists, it must not be recreated: this would restart the flash
// animation.
pub fn update_chat(chat_node: &web_sys::Element, items: &[ChatItem]) -> JsResult<()> {
    let mut items_added = false;
    let mut idx = 0;
    while idx < chat_node.children().length() && (idx as usize) < items.len() {
        let item = &items[idx as usize];
        let child_node = chat_node.children().get_with_index(idx).unwrap();
        let child_id = child_node.get_attribute(CHAT_ID_ATTR).ok_or_else(|| rust_error!())?;
        if child_id < item.id {
            chat_node.children().get_with_index(idx).unwrap().remove();
        } else if child_id > item.id {
            items_added = true;
            let new_node = new_chat_item(item)?;
            chat_node.insert_before(&new_node, Some(&child_node))?;
            idx += 1;
        } else {
            update_chat_item(&child_node, item)?;
            idx += 1;
        }
    }
    while idx < chat_node.children().length() {
        chat_node.children().get_with_index(idx).unwrap().remove();
    }
    while (idx as usize) < items.len() {
        items_added = true;
        let item = &items[idx as usize];
        let new_node = new_chat_item(item)?;
        chat_node.append_child(&new_node)?;
        idx += 1;
    }
    if items_added {
        // TODO: Scroll to bottom only if it was at the bottom.
        scroll_to_bottom(&chat_node);
    }
    Ok(())
}

// Assumptions:
//   - item sender and recipient are never changed;
//   - item text is never changed.
fn update_chat_item(item_node: &web_sys::Element, item: &ChatItem) -> JsResult<()> {
    item_node.class_list().toggle_with_force(
        "chat-message-unconfirmed",
        item.durability == ChatItemDurability::Local,
    )?;
    item_node.class_list().toggle_with_force("chat-message-dimmed", item.dimmed)?;
    item_node
        .class_list()
        .toggle_with_force("chat-message-prominent", item.prominent)?;
    item_node.class_list().toggle_with_force("chat-message-flash", item.flash)?;
    Ok(())
}

fn new_chat_item(item: &ChatItem) -> JsResult<web_sys::Element> {
    let document = web_document();

    let item_node = document.create_element("div")?;
    item_node.set_attribute(CHAT_ID_ATTR, &item.id)?;
    item_node.class_list().add_1("chat-message")?;

    if item.sender.is_some() || item.recipient.is_some() {
        let prefix_node = item_node.append_span(["chat-prefix"])?;
        if let Some(sender) = &item.sender {
            let (sender_name, sender_class) = chat_party_name_and_class(&sender, "sender");
            prefix_node.append_text_span(sender_name, ["chat-sender", &sender_class])?;
        }
        if let Some(recipient) = &item.recipient {
            let (recipient_name, recipient_class) =
                chat_party_name_and_class(&recipient, "recipient");
            prefix_node.append_text_span("→", ["chat-arrow"])?;
            prefix_node.append_text_span(recipient_name, ["chat-recipient", &recipient_class])?;
        }
        prefix_node.append_text_span(":", ["chat-colon"])?;
    }

    item_node.append_text_span(&item.text, ["chat-message-text"])?;

    update_chat_item(&item_node, item)?;
    Ok(item_node)
}

fn chat_party_name_and_class<'a>(party: &'a ChatParty, side: &str) -> (&'a str, String) {
    match party {
        ChatParty::Myself => ("me", format!("chat-{side}-myself")),
        ChatParty::Participant(name) => (name, format!("chat-{side}-participant")),
        ChatParty::All => ("all", format!("chat-{side}-all")),
        ChatParty::System(message_class) => {
            let message_class = match message_class {
                SystemMessageClass::Info => "info",
                SystemMessageClass::Error => "error",
                SystemMessageClass::GameOver => "game-over",
            };
            ("system", format!("chat-{side}-system-{message_class}"))
        }
    }
}

enum ChatReferenceElement {
    Command(&'static str),
    Message(&'static str),
    Recipient(&'static str),
    Notation(&'static str),
}

pub fn render_chat_reference_tooltip(
    participant_id: BughouseParticipant, team_chat_enabled: bool,
) -> JsResult<()> {
    use ChatReferenceElement::*;
    let mut lines = vec![];

    if team_chat_enabled {
        lines.push(([Message("message")].as_slice(), "send to team"));
        lines.push(([Command("/a"), Message(" message")].as_slice(), "send to all"));
    } else {
        lines.push(([Message("message")].as_slice(), "send to all"));
    }
    lines.push((
        [Command("/dm"), Recipient(" name"), Message(" message")].as_slice(),
        "send to a given player",
    ));

    lines.push(([Command("/h")].as_slice(), "more details on chat commands"));

    match participant_id {
        BughouseParticipant::Observer => {}
        BughouseParticipant::Player(BughousePlayer::SinglePlayer(_)) => {
            lines.push((
                [Command("<"), Notation("notation")].as_slice(),
                "make move with algebraic notation",
            ));
        }
        BughouseParticipant::Player(BughousePlayer::DoublePlayer(_)) => {
            lines.push((
                [Command("<"), Notation("notation")].as_slice(),
                "make move on the left board",
            ));
            lines.push((
                [Command(">"), Notation("notation")].as_slice(),
                "make move on the right board",
            ));
        }
    }

    let document = web_document();
    let reference_node = document.get_existing_element_by_id("chat-reference-tooltip")?;
    remove_all_children(&reference_node)?;
    for line in lines {
        let (input, explanation) = line;
        let line_node = reference_node.append_new_element("div")?;
        let input_node = line_node.append_span(["chat-reference-input"])?;
        for element in input {
            let (text, class) = match element {
                Command(text) => (text, "chat-reference-command"),
                Message(text) => (text, "chat-reference-message"),
                Recipient(text) => (text, "chat-reference-recipient"),
                Notation(text) => (text, "chat-reference-notation"),
            };
            input_node.append_text_span(*text, [class])?;
        }
        line_node.append_text_span(" — ", ["chat-reference-separator"])?;
        line_node.append_text_span(explanation, ["chat-reference-explanation"])?;
    }
    Ok(())
}

pub fn render_chat_reference_dialog() -> JsResult<()> {
    use ChatReferenceElement::*;
    // Improvement potential. Highlight "/a" and notation examples in explanation sections.
    let mut line_groups = vec![];
    line_groups.push(vec![
        (
            [Message("message")].as_slice(),
            [
                "Send a message to the team if playing in a team.",
                "Acts as /a if playing on two boards or observing.",
            ]
            .as_slice(),
        ),
        (
            [Command("/a"), Message(" message")].as_slice(),
            ["Send a message to all players and observers, including those who join later."]
                .as_slice(),
        ),
        (
            [Command("/dm"), Recipient(" name"), Message(" message")].as_slice(),
            ["Send a message to a given player."].as_slice(),
        ),
    ]);
    line_groups.push(vec![
        ([Command("/resign")].as_slice(), ["Resign from the game."].as_slice()),
        (
            [Command("/ready")].as_slice(),
            ["Toggle readiness for the next game."].as_slice(),
        ),
        ([Command("/h")].as_slice(), ["Show this reference."].as_slice()),
        (
            [Command("/tooltip")].as_slice(),
            ["Toggle tooltip with the short version of this reference."].as_slice(),
        ),
    ]);
    line_groups.push(vec![
        (
            [Command("<"), Notation("notation")].as_slice(),
            ["Make move (on the left board if playing on two boards)."].as_slice(),
        ),
        (
            [Command("<"), Notation("-")].as_slice(),
            ["Undo premove (on the left board if playing on two boards)."].as_slice(),
        ),
        (
            [Command(">"), Notation("notation")].as_slice(),
            ["Make move on the right board (if playing on two boards)."].as_slice(),
        ),
        (
            [Command(">"), Notation("-")].as_slice(),
            ["Undo premove on the right board (if playing on two boards)."].as_slice(),
        ),
    ]);
    line_groups.push(vec![(
        [].as_slice(),
        [
            "Algebraic notation reference:",
            "Use standard notation for chess moves: e4 (or e2e4), Nc3, Qxd5, Rad8, O-O (or 0-0).",
            "Drops are denoted by piece name followed by “@” followed by target square: P@c6, B@f3.",
            "Duck moves are denoted by “@” followed by target square: @e3.",
            "Promotions are denoted with “/” or “=”:",
            "- regular (upgrade) promotions: e8/Q (or e8=Q).",
            "- discard promotions: e8/. (or e8=.).",
            "- steal promotions: e8/Rc1 (or e8/Rc1); note that the target square is on the other board.",
        ]
        .as_slice(),
    )]);

    let document = web_document();
    let reference_node = document.get_existing_element_by_id("chat-reference-dialog-body")?;
    remove_all_children(&reference_node)?;
    let table = reference_node.append_new_element("table")?;
    for (group_index, group) in line_groups.iter().enumerate() {
        if group_index > 0 {
            table
                .append_new_element("tr")?
                .with_classes(["chat-reference-group-separator"])?;
        }
        for line in group {
            let (input, explanation) = line;
            let line_tr = table.append_new_element("tr")?;
            let input_td =
                line_tr.append_new_element("td")?.with_classes(["chat-reference-input"])?;
            for element in input.iter() {
                let (text, class) = match element {
                    Command(text) => (text, "chat-reference-command"),
                    Message(text) => (text, "chat-reference-message"),
                    Recipient(text) => (text, "chat-reference-recipient"),
                    Notation(text) => (text, "chat-reference-notation"),
                };
                input_td.append_text_span(*text, [class])?;
            }
            let explanation_td =
                line_tr.append_new_element("td")?.with_classes(["chat-reference-explanation"])?;
            for (i, l) in explanation.iter().enumerate() {
                if i > 0 {
                    explanation_td.append_new_element("br")?;
                }
                explanation_td.append_text_span(l, [])?;
            }
        }
    }
    Ok(())
}
