use std::ops;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::JsCast;

use crate::web_document::web_document;
use crate::web_error_handling::JsResult;


pub trait WebElementExt {
    fn with_id(self, value: &str) -> web_sys::Element;
    fn with_maybe_text_content(self, text: Option<&str>) -> web_sys::Element;
    fn with_text_content(self, text: &str) -> web_sys::Element;
    // TODO: Use it where appropriate.
    // TODO: Use our beautiful tooltips instead. Need to figure out how to fix them for elements
    // inside `overflow: hidden` containers.
    fn with_title(self, title_text: &str) -> JsResult<web_sys::Element>;
    fn with_attribute(self, name: &str, value: &str) -> JsResult<web_sys::Element>;
    fn with_classes(self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element>;

    fn is_displayed(&self) -> bool;
    fn set_displayed(&self, displayed: bool) -> JsResult<()>;

    fn add_event_listener_and_forget<E: FromWasmAbi + 'static>(
        &self, event_type: &str, listener: impl FnMut(E) -> JsResult<()> + 'static,
    ) -> JsResult<()>;

    fn remove_all_children(&self);
    fn set_children(
        &self, children: impl IntoIterator<Item = impl ops::Deref<Target = web_sys::Node>>,
    ) -> JsResult<()>;

    fn append_element(&self, child: web_sys::Element) -> JsResult<()>;
    fn append_children(
        &self, children: impl IntoIterator<Item = impl ops::Deref<Target = web_sys::Node>>,
    ) -> JsResult<()>;
    fn append_new_element(&self, local_name: &str) -> JsResult<web_sys::Element>;
    fn append_new_svg_element(&self, local_name: &str) -> JsResult<web_sys::Element>;

    // TODO: More consistent test builder API. We sometimes return `self` and sometimes the appended
    // element. This is confusing. Consider:
    //   - Always prefix functions returning `self` with `with_`.
    //   - Always prefix functions returning the appended element with something else.
    fn append_text(self, text: &str) -> JsResult<web_sys::Element>;
    fn append_text_i(self, text: &str) -> JsResult<web_sys::Element>;
    fn append_span(&self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element>;
    fn append_text_span(&self, text: &str, classes: impl IntoIterator<Item = &str>)
        -> JsResult<()>;
    fn append_animated_dots(&self) -> JsResult<()>;
}

impl WebElementExt for web_sys::Element {
    fn with_id(self, value: &str) -> web_sys::Element {
        self.set_id(value);
        self
    }

    fn with_maybe_text_content(self, text: Option<&str>) -> web_sys::Element {
        self.set_text_content(text);
        self
    }

    fn with_text_content(self, text: &str) -> web_sys::Element {
        self.with_maybe_text_content(Some(text))
    }

    fn with_title(self, title_text: &str) -> JsResult<web_sys::Element> {
        self.with_attribute("title", title_text)
    }

    fn with_attribute(self, name: &str, value: &str) -> JsResult<web_sys::Element> {
        self.set_attribute(name, value)?;
        Ok(self)
    }

    fn with_classes(self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element> {
        for class in classes {
            self.class_list().add_1(class)?;
        }
        Ok(self)
    }

    fn is_displayed(&self) -> bool { !self.class_list().contains("display-none") }

    // TODO: Sync with `set_displayed` in `index.js`: either always use `display-none` class or
    // always set `display` attribute directly.
    fn set_displayed(&self, displayed: bool) -> JsResult<()> {
        self.class_list().toggle_with_force("display-none", !displayed)?;
        Ok(())
    }

    // TODO: Don't leak, let JS GC handle it. In order to GC the closure when the element is deleted
    // we could try something like:
    // ```
    //     Reflect::set(
    //         &self,
    //         &some_unique_key,
    //         &closure.into_js_value()
    //     ).unwrap();
    // ```
    // although admittedly this is a bit of a hack. I haven't yet found a way to automatically GC
    // the closure when the event listener is removed. It's weird web_sys doesn't provide way to
    // deal with this out-of-the-box.
    fn add_event_listener_and_forget<E: FromWasmAbi + 'static>(
        &self, event_type: &str, listener: impl FnMut(E) -> JsResult<()> + 'static,
    ) -> JsResult<()> {
        let closure = Closure::new(listener);
        self.add_event_listener_with_callback(event_type, closure.as_ref().unchecked_ref())?;
        closure.forget();
        Ok(())
    }

    fn remove_all_children(&self) { self.replace_children_with_node_0() }

    fn set_children(
        &self, children: impl IntoIterator<Item = impl ops::Deref<Target = web_sys::Node>>,
    ) -> JsResult<()> {
        self.remove_all_children();
        self.append_children(children)
    }

    // Workaround for not being able to call `append_child(func_returning_element()?)` without an
    // intermediate variable.
    fn append_element(&self, child: web_sys::Element) -> JsResult<()> {
        self.append_child(&child)?;
        Ok(())
    }

    fn append_children(
        &self, children: impl IntoIterator<Item = impl ops::Deref<Target = web_sys::Node>>,
    ) -> JsResult<()> {
        for child in children {
            self.append_child(&child)?;
        }
        Ok(())
    }

    // Improvement potential. Check if fetching `web_document()` every time slows down functions
    // that create a lot of elements (here and elsewhere).
    fn append_new_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        let node = web_document().create_element(local_name)?;
        self.append_child(&node)?;
        Ok(node)
    }

    fn append_new_svg_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        let node = web_document().create_svg_element(local_name)?;
        self.append_child(&node)?;
        Ok(node)
    }

    fn append_text(self, text: &str) -> JsResult<web_sys::Element> {
        self.append_with_str_1(text)?;
        Ok(self)
    }

    fn append_text_i(self, text: &str) -> JsResult<web_sys::Element> {
        let i = self.append_new_element("i")?;
        i.append_text(text)?;
        Ok(self)
    }

    fn append_span(&self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element> {
        self.append_new_element("span")?.with_classes(classes)
    }

    fn append_text_span(
        &self, text: &str, classes: impl IntoIterator<Item = &str>,
    ) -> JsResult<()> {
        let span = self.append_span(classes)?;
        span.set_text_content(Some(text));
        Ok(())
    }

    // TODO: Deduplicate. This is also implemented in `index.js` as `make_animated_dots` and
    // explicitly spelled out in `index.html`.
    fn append_animated_dots(&self) -> JsResult<()> {
        for _ in 0..3 {
            self.append_new_element("span")?.with_classes(["dot"])?.with_text_content(".");
        }
        Ok(())
    }
}

// TODO: Show error content when the callback passed to `add_event_listener_and_forget` fails.
// I tried these approached, but they didn't work:
//
// fn debug_js_value(js_value: JsValue) -> String {
//     let json_value: Result<serde_json::Value, _> = serde_wasm_bindgen::from_value(js_value);
//     match json_value {
//         Ok(json_value) => {
//             match serde_json::to_string(&json_value) {
//                 Ok(json_string) => json_string,
//                 Err(err) => format!("Error serializing to JSON: {}", err),
//             }
//         }
//         Err(err) => format!("Error converting JsValue: {}", err),
//     }
// }
//
// #[wasm_bindgen(inline_js = "export function json_stringify(obj) {
//     return JSON.stringify(obj, null, 2);
// }")]
// extern "C" {
//     fn json_stringify(obj: &JsValue) -> String;
// }
// #[wasm_bindgen]
// pub fn debug_js_value(js_value: JsValue) -> String { json_stringify(&js_value) }
