use crate::html_collection_iterator::IntoHtmlCollectionIterator;
use crate::rust_error;
use crate::web_document::web_document;
use crate::web_error_handling::JsResult;
use crate::web_util::scroll_to_bottom;


// TODO: Find a better alternative to ephemeral messages.
pub struct ChatMessage {
    text: String,
    sender: Option<(String, String)>, // class and name
    prominent: bool,                  // message will be bolder and more prominent
    ephemeral: bool,                  // message will be removed when another message is added
    flash: bool,                      // message will be highlighted upon addition
}

impl ChatMessage {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            sender: None,
            prominent: false,
            ephemeral: false,
            flash: false,
        }
    }

    pub fn with_sender(
        mut self, sender_name: impl Into<String>, sender_class: impl Into<String>,
    ) -> Self {
        self.sender = Some((sender_name.into(), sender_class.into()));
        self
    }
    pub fn with_prominent(mut self) -> Self {
        self.prominent = true;
        self
    }
    pub fn with_ephemeral(mut self) -> Self {
        self.ephemeral = true;
        self
    }
    pub fn with_flash(mut self) -> Self {
        self.flash = true;
        self
    }
}

pub fn clear_ephemeral_chat_messages(chat_node: &web_sys::Element) -> JsResult<()> {
    for child_node in chat_node.get_elements_by_class_name("chat-message-ephemeral").into_iterator()
    {
        child_node.remove();
    }
    Ok(())
}

pub fn add_chat_message(chat_node: &web_sys::Element, message: ChatMessage) -> JsResult<()> {
    if message.text.is_empty() {
        return Err(rust_error!());
    }

    let document = web_document();
    clear_ephemeral_chat_messages(chat_node)?;

    let message_node = document.create_element("div")?;
    message_node.class_list().add_1("chat-message")?;
    if message.ephemeral {
        message_node.class_list().add_1("chat-message-ephemeral")?;
    }
    if message.flash {
        message_node.class_list().add_1("chat-message-flash")?;
    }

    if let Some((sender_name, sender_class)) = message.sender {
        let sender_node = document.create_element("span")?;
        sender_node.class_list().add_2("chat-sender", &sender_class)?;
        sender_node.set_text_content(Some(&sender_name));
        message_node.append_child(&sender_node)?;
    }

    let text_node = document.create_element("span")?;
    text_node.set_text_content(Some(&message.text));
    if message.prominent {
        text_node.class_list().add_1("chat-message-prominent")?;
    }

    message_node.append_child(&text_node)?;
    chat_node.append_child(&message_node)?;
    // TODO: Scroll to bottom only if it was at the bottom.
    scroll_to_bottom(&chat_node);
    Ok(())
}
