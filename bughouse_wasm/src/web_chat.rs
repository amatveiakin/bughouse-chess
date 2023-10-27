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

use crate::rust_error;
use crate::web_document::web_document;
use crate::web_error_handling::JsResult;
use crate::web_util::scroll_to_bottom;


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
        } else {
            update_chat_item(&child_node, item)?;
        }
        idx += 1;
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
        let prefix_node = document.create_element("span")?;
        prefix_node.class_list().add_1("chat-prefix")?;
        if let Some(sender) = &item.sender {
            let (sender_name, sender_class) = chat_party_name_and_class(&sender, "sender");
            let node = document.create_element("span")?;
            node.class_list().add_2("chat-sender", &sender_class)?;
            node.set_text_content(Some(&sender_name));
            prefix_node.append_child(&node)?;
        }
        if let Some(recipient) = &item.recipient {
            let (recipient_name, recipient_class) =
                chat_party_name_and_class(&recipient, "recipient");
            {
                let node = document.create_element("span")?;
                node.class_list().add_1("chat-arrow")?;
                node.set_text_content(Some("→"));
                prefix_node.append_child(&node)?;
            }
            {
                let node = document.create_element("span")?;
                node.class_list().add_2("chat-recipient", &recipient_class)?;
                node.set_text_content(Some(&recipient_name));
                prefix_node.append_child(&node)?;
            }
        }
        {
            let node = document.create_element("span")?;
            node.class_list().add_1("chat-colon")?;
            node.set_text_content(Some(":"));
            prefix_node.append_child(&node)?;
        }
        item_node.append_child(&prefix_node)?;
    }

    let text_node = document.create_element("span")?;
    text_node.class_list().add_1("chat-message-text")?;
    text_node.set_text_content(Some(&item.text));
    item_node.append_child(&text_node)?;

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
