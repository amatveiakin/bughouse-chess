pub fn scroll_to_bottom(e: &web_sys::Element) {
    // Do not try to compute the real scroll position, as it is very slow!
    // See the comment in `update_turn_log`.
    e.set_scroll_top(1_000_000_000);
}
