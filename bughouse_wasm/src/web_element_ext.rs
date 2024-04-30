use std::ops;

use crate::web_document::web_document;
use crate::web_error_handling::JsResult;


pub trait WebElementExt {
    fn with_id(self, value: &str) -> web_sys::Element;
    fn with_text_content(self, text: &str) -> web_sys::Element;
    fn with_attribute(self, name: &str, value: &str) -> JsResult<web_sys::Element>;
    fn with_classes(self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element>;

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
    // element. This is confusing.
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

    fn with_text_content(self, text: &str) -> web_sys::Element {
        self.set_text_content(Some(text));
        self
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

    fn append_animated_dots(&self) -> JsResult<()> {
        for _ in 0..3 {
            self.append_new_element("span")?.with_classes(["dot"])?.with_text_content(".");
        }
        Ok(())
    }
}
