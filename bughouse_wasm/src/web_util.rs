use crate::web_error_handling::JsResult;


pub fn remove_all_children(node: &web_sys::Node) -> JsResult<()> {
    // TODO: Consider: replace_children_with_node_0.
    while let Some(child) = node.last_child() {
        node.remove_child(&child)?;
    }
    Ok(())
}

pub fn scroll_to_bottom(e: &web_sys::Element) {
    // Do not try to compute the real scroll position, as it is very slow!
    // See the comment in `update_turn_log`.
    e.set_scroll_top(1_000_000_000);
}
