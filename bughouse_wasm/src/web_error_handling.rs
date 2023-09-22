use std::cell::RefCell;

use bughouse_chess::{BughouseClientErrorReport, BughouseClientEvent};
use wasm_bindgen::prelude::*;


pub type JsResult<T> = Result<T, JsValue>;

// The client is single-threaded, so wrapping all mutable singletons in `thread_local!` seems ok.
thread_local! {
    static LAST_PANIC: RefCell<String> = RefCell::new(String::new());
}

// Copied from console_error_panic_hook
#[wasm_bindgen]
extern "C" {
    type Error;
    #[wasm_bindgen(constructor)]
    fn new() -> Error;
    #[wasm_bindgen(structural, method, getter)]
    fn stack(error: &Error) -> String;
}

// Optimization potential: Remove or shrink the panic hook when the client is stable.
#[wasm_bindgen]
pub fn set_panic_hook() {
    use std::panic;
    use std::sync::Once;
    static SET_HOOK: Once = Once::new();
    SET_HOOK.call_once(|| {
        panic::set_hook(Box::new(|panic_info| {
            // Log to the browser developer console. For more details see
            // https://github.com/rustwasm/console_error_panic_hook#readme
            console_error_panic_hook::hook(panic_info);

            // Generate error report to be sent to the server.
            let js_error = Error::new();
            let backtrace = js_error.stack();
            let event = BughouseClientEvent::ReportError(BughouseClientErrorReport::RustPanic {
                panic_info: panic_info.to_string(),
                backtrace,
            });
            LAST_PANIC.with(|cell| *cell.borrow_mut() = serde_json::to_string(&event).unwrap());
        }));
    });
}

#[wasm_bindgen]
pub fn last_panic() -> String { LAST_PANIC.with(|cell| cell.borrow().clone()) }

#[wasm_bindgen(getter_with_clone)]
pub struct RustError {
    pub message: String,
}

#[macro_export]
macro_rules! rust_error {
    ($($arg:tt)*) => {
        wasm_bindgen::JsValue::from(
            $crate::web_error_handling::RustError{ message: format!($($arg)*) }
        )
    };
}

#[wasm_bindgen]
pub fn make_rust_error_event(error: RustError) -> String {
    let event = BughouseClientEvent::ReportError(BughouseClientErrorReport::RustError {
        message: error.message,
    });
    serde_json::to_string(&event).unwrap()
}

#[wasm_bindgen]
pub fn make_unknown_error_event(message: String) -> String {
    let event =
        BughouseClientEvent::ReportError(BughouseClientErrorReport::UnknownError { message });
    serde_json::to_string(&event).unwrap()
}
