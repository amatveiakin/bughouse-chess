use std::ops;

use strum::EnumIter;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::convert::FromWasmAbi;

use crate::rust_error;
use crate::web_document::web_document;
use crate::web_error_handling::JsResult;
use crate::web_iterators::HtmlCollectionIterator;


#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
pub enum TooltipPosition {
    Right,
    Above,
    Below,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
pub enum TooltipWidth {
    Auto,
    M,
    L,
}

impl TooltipPosition {
    pub fn to_class_name(self) -> &'static str {
        match self {
            TooltipPosition::Right => "tooltip-right",
            TooltipPosition::Above => "tooltip-above",
            TooltipPosition::Below => "tooltip-below",
        }
    }
}

impl TooltipWidth {
    pub fn to_class_name(self) -> &'static str {
        match self {
            TooltipWidth::Auto => "tooltip-width-auto",
            TooltipWidth::M => "tooltip-width-m",
            TooltipWidth::L => "tooltip-width-l",
        }
    }
}

// Name convention for functions returning `web_sys::Element`:
//   - `with_` prefix: functions returning `self`,
//   - `get_` prefix: functions that find existing child elements,
//   - `new_child_` prefix: functions creating a new child element and returning it,
//   - other functions should not return a `web_sys::Element`.
pub trait WebElementExt {
    fn with_id(self, value: &str) -> web_sys::Element;
    fn with_maybe_text_content(self, text: Option<&str>) -> web_sys::Element;
    fn with_text_content(self, text: &str) -> web_sys::Element;
    fn with_more_text(self, text: &str) -> JsResult<web_sys::Element>;
    fn with_more_text_i(self, text: &str) -> JsResult<web_sys::Element>;
    fn with_attribute(self, name: &str, value: &str) -> JsResult<web_sys::Element>;
    fn with_classes(self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element>;
    // Do not use portal tooltips on temporary elements! (see `new_child_portal_tooltip`)
    fn with_plaintext_portal_tooltip(
        self, position: TooltipPosition, width: TooltipWidth, text: &str,
    ) -> JsResult<web_sys::Element>;
    fn with_children_removed(self) -> web_sys::Element;

    fn is_displayed(&self) -> bool;
    fn set_displayed(&self, displayed: bool) -> JsResult<()>;

    fn get_unique_element_by_tag_name(&self, local_name: &str) -> JsResult<web_sys::Element>;
    fn get_elements_by_class_name_iter(&self, class_name: &str) -> HtmlCollectionIterator;

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
    fn append_text_span(&self, text: &str, classes: impl IntoIterator<Item = &str>)
    -> JsResult<()>;
    fn append_animated_dots(&self) -> JsResult<()>;

    fn new_child_element(&self, local_name: &str) -> JsResult<web_sys::Element>;
    fn new_child_svg_element(&self, local_name: &str) -> JsResult<web_sys::Element>;
    fn new_child_tooltip(
        &self, position: TooltipPosition, width: TooltipWidth,
    ) -> JsResult<web_sys::Element>;
    // Produces the same visual result as `new_child_tooltip`, but uses the portal approach to
    // escape parent element clipping. Prefer `new_child_tooltip` when possible.
    // Do not use portal tooltips on temporary elements! There is currently no mechanism to clean up
    // the tooltip when the element is deleted. This is always a memory leak and in the worst case
    // the tooltip could get stuck in the displayed state until page refresh.
    fn new_child_portal_tooltip(
        &self, position: TooltipPosition, width: TooltipWidth,
    ) -> JsResult<web_sys::Element>;
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

    fn with_more_text(self, text: &str) -> JsResult<web_sys::Element> {
        self.append_with_str_1(text)?;
        Ok(self)
    }

    fn with_more_text_i(self, text: &str) -> JsResult<web_sys::Element> {
        let i = self.new_child_element("i")?;
        i.with_more_text(text)?;
        Ok(self)
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

    fn with_plaintext_portal_tooltip(
        self, position: TooltipPosition, width: TooltipWidth, text: &str,
    ) -> JsResult<web_sys::Element> {
        let tooltip_node = self.new_child_portal_tooltip(position, width)?;
        tooltip_node
            .new_child_element("p")?
            .with_classes(["ws-pre-line"])?
            .with_text_content(text);
        Ok(self)
    }

    fn with_children_removed(self) -> web_sys::Element {
        self.remove_all_children();
        self
    }

    fn is_displayed(&self) -> bool { !self.class_list().contains("display-none") }

    // TODO: Sync with `set_displayed` in `index.js`: either always use `display-none` class or
    // always set `display` attribute directly.
    fn set_displayed(&self, displayed: bool) -> JsResult<()> {
        self.class_list().toggle_with_force("display-none", !displayed)?;
        Ok(())
    }

    fn get_unique_element_by_tag_name(&self, local_name: &str) -> JsResult<web_sys::Element> {
        let collection = self.get_elements_by_tag_name(local_name);
        match collection.length() {
            0 => Err(rust_error!("Cannot find element with tag name \"{}\"", local_name)),
            1 => Ok(collection.get_with_index(0).unwrap()),
            _ => Err(rust_error!("Expected exactly one element with tag name \"{}\"", local_name)),
        }
    }

    fn get_elements_by_class_name_iter(&self, class_name: &str) -> HtmlCollectionIterator {
        self.get_elements_by_class_name(class_name).into()
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

    fn append_text_span(
        &self, text: &str, classes: impl IntoIterator<Item = &str>,
    ) -> JsResult<()> {
        let span = self.new_child_element("span")?.with_classes(classes)?;
        span.set_text_content(Some(text));
        Ok(())
    }

    // TODO: Deduplicate. This is also implemented in `index.js` as `make_animated_dots` and
    // explicitly spelled out in `index.html`.
    fn append_animated_dots(&self) -> JsResult<()> {
        for _ in 0..3 {
            self.new_child_element("span")?.with_classes(["dot"])?.with_text_content(".");
        }
        Ok(())
    }

    // Improvement potential. Check if fetching `web_document()` every time slows down functions
    // that create a lot of elements (here and elsewhere).
    fn new_child_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        let node = web_document().create_element(local_name)?;
        self.append_child(&node)?;
        Ok(node)
    }

    fn new_child_svg_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        let node = web_document().create_svg_element(local_name)?;
        self.append_child(&node)?;
        Ok(node)
    }

    // TODO: Dedup with `set_tooltip` in `index.js`.
    fn new_child_tooltip(
        &self, position: TooltipPosition, width: TooltipWidth,
    ) -> JsResult<web_sys::Element> {
        self.class_list().add_1("tooltip-container")?;
        for tooltip_node in self.get_elements_by_class_name_iter("tooltip-text") {
            tooltip_node.remove();
        }
        self.new_child_element("div")?.with_classes([
            "tooltip-text",
            position.to_class_name(),
            width.to_class_name(),
        ])
    }

    fn new_child_portal_tooltip(
        &self, position: TooltipPosition, width: TooltipWidth,
    ) -> JsResult<web_sys::Element> {
        if self.get_elements_by_class_name("tooltip-text").length() > 0 {
            // Shouldn't call many times, because we leak callbacks.
            // TODO: Fix closure leaks and allow to change tooltips.
            return Err(rust_error!("`with_tooltip` must be called at most once"));
        }
        let tooltip_node = web_document().body()?.new_child_element("div")?.with_classes([
            "tooltip-text",
            position.to_class_name(),
            width.to_class_name(),
        ])?;
        let parent = self.clone();
        let tooltip_node1 = tooltip_node.clone().dyn_into::<web_sys::HtmlElement>()?;
        let tooltip_node2 = tooltip_node1.clone();
        self.add_event_listener_and_forget("mouseenter", move |_: web_sys::Event| {
            // https://developer.mozilla.org/en-US/docs/Web/API/Element/getBoundingClientRect quote:
            // "If you need the bounding rectangle relative to the top-left corner of the document,
            // just add the current scrolling position to the top and left properties (these can be
            // obtained using window.scrollY and window.scrollX) to get a bounding rectangle which
            // is independent from the current scrolling position."
            let window = web_sys::window().unwrap();
            let parent_rect = parent.get_bounding_client_rect();
            let (mut x, mut y) = match position {
                TooltipPosition::Right => {
                    (parent_rect.right(), parent_rect.top() + parent_rect.height() / 2.0)
                }
                TooltipPosition::Above => {
                    (parent_rect.left() + parent_rect.width() / 2.0, parent_rect.top())
                }
                TooltipPosition::Below => {
                    (parent_rect.left() + parent_rect.width() / 2.0, parent_rect.bottom())
                }
            };
            x += window.scroll_x()?;
            y += window.scroll_y()?;
            set_coordinates(&tooltip_node1, x, y)?;
            tooltip_node1.class_list().add_1("tooltip-force-show")?;
            Ok(())
        })?;
        self.add_event_listener_and_forget("mouseleave", move |_: web_sys::Event| {
            tooltip_node2.class_list().remove_1("tooltip-force-show")?;
            Ok(())
        })?;
        Ok(tooltip_node)
    }
}

fn set_coordinates(node: &web_sys::HtmlElement, x: f64, y: f64) -> JsResult<()> {
    let style = node.style();
    style.set_property("left", &format!("{}px", x))?;
    style.set_property("top", &format!("{}px", y))?;
    Ok(())
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
