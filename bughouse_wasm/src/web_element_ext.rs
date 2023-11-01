use crate::web_document::web_document;
use crate::web_error_handling::JsResult;


pub trait WebElementExt {
    fn with_classes(self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element>;
    fn append_new_element(&self, local_name: &str) -> JsResult<web_sys::Element>;
    fn append_span(&self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element>;
    fn append_text_span(&self, text: &str, classes: impl IntoIterator<Item = &str>)
        -> JsResult<()>;
}

impl WebElementExt for web_sys::Element {
    fn with_classes(self, classes: impl IntoIterator<Item = &str>) -> JsResult<web_sys::Element> {
        for class in classes {
            self.class_list().add_1(class)?;
        }
        Ok(self)
    }

    // Improvement potential. Check if fetching `web_document()` every time slows down functions
    // that create a lot of elements.
    fn append_new_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        let node = web_document().create_element(local_name)?;
        self.append_child(&node)?;
        Ok(node)
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
}
