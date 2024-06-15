use wasm_bindgen::prelude::*;

use crate::rust_error;
use crate::web_document::web_document;
use crate::web_error_handling::JsResult;

pub fn scroll_to_bottom(e: &web_sys::Element) {
    // Do not try to compute the real scroll position, as it is very slow!
    // See the comment in `update_turn_log`.
    e.set_scroll_top(1_000_000_000);
}

// Slow! Use `estimate_text_width` when precise result is not needed.
#[allow(unused)]
pub fn get_text_width(s: &str) -> JsResult<u32> {
    let canvas = web_document()
        .get_existing_element_by_id("canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let context = canvas
        .get_context("2d")?
        .ok_or_else(|| rust_error!("Canvas 2D context missing"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;
    Ok(context.measure_text(s)?.width() as u32)
}

pub fn estimate_text_width(s: &str) -> JsResult<u32> {
    const DEFAULT_WIDTH: u32 = 5;
    const CHAR_WIDTHS: [u32; 95] = [
        2,  // space
        2,  // !
        3,  // "
        5,  // #
        5,  // $
        8,  // %
        6,  // &
        1,  // '
        3,  // (
        3,  // )
        3,  // *
        5,  // +
        2,  // ,
        3,  // -
        2,  // .
        2,  // /
        5,  // 0
        5,  // 1
        5,  // 2
        5,  // 3
        5,  // 4
        5,  // 5
        5,  // 6
        5,  // 7
        5,  // 8
        5,  // 9
        2,  // :
        2,  // ;
        5,  // <
        5,  // =
        5,  // >
        5,  // ?
        10, // @
        6,  // A
        6,  // B
        7,  // C
        7,  // D
        6,  // E
        6,  // F
        7,  // G
        7,  // H
        2,  // I
        5,  // J
        6,  // K
        5,  // L
        8,  // M
        7,  // N
        7,  // O
        6,  // P
        7,  // Q
        7,  // R
        6,  // S
        6,  // T
        7,  // U
        6,  // V
        9,  // W
        6,  // X
        6,  // Y
        6,  // Z
        2,  // [
        2,  // \
        2,  // ]
        4,  // ^
        5,  // _
        3,  // `
        5,  // a
        5,  // b
        5,  // c
        5,  // d
        5,  // e
        2,  // f
        5,  // g
        5,  // h
        2,  // i
        2,  // j
        5,  // k
        2,  // l
        8,  // m
        5,  // n
        5,  // o
        5,  // p
        5,  // q
        3,  // r
        5,  // s
        2,  // t
        5,  // u
        5,  // v
        7,  // w
        5,  // x
        5,  // y
        5,  // z
        3,  // {
        2,  // |
        3,  // }
        5,  // ~
    ];
    Ok(s.chars()
        .map(|ch| CHAR_WIDTHS.get(ch as usize - 0x20).copied().unwrap_or(DEFAULT_WIDTH))
        .sum())
}

// Width table generator for `estimate_text_width`:
// #[wasm_bindgen]
// pub fn estimate_ascii_char_widths() -> JsResult<String> {
//     let mut ret = String::new();
//     for ch in b' '..=b'~' {
//         let ch = ch as char;
//         ret += &format!("{ch}: {}\n", get_text_width(&ch.to_string())?);
//     }
//     Ok(ret)
// }
