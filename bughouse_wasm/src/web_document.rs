use crate::html_collection_iterator::HtmlCollectionIterator;
use crate::rust_error;
use crate::web_error_handling::JsResult;


pub struct WebDocument(web_sys::Document);

impl WebDocument {
    pub fn body(&self) -> JsResult<web_sys::HtmlElement> {
        self.0.body().ok_or_else(|| rust_error!("Cannot find document body"))
    }

    pub fn get_element_by_id(&self, element_id: &str) -> Option<web_sys::Element> {
        self.0.get_element_by_id(element_id)
    }
    pub fn get_existing_element_by_id(&self, element_id: &str) -> JsResult<web_sys::Element> {
        let element = self
            .0
            .get_element_by_id(element_id)
            .ok_or_else(|| rust_error!("Cannot find element \"{}\"", element_id))?;
        if !element.is_object() {
            return Err(rust_error!("Element \"{}\" is not an object", element_id));
        }
        Ok(element)
    }

    pub fn get_elements_by_class_name(&self, class_name: &str) -> HtmlCollectionIterator {
        self.0.get_elements_by_class_name(class_name).into()
    }

    pub fn create_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        self.0.create_element(local_name)
    }
    pub fn create_svg_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        self.0.create_element_ns(Some("http://www.w3.org/2000/svg"), local_name)
    }
}

pub fn web_document() -> WebDocument { WebDocument(web_sys::window().unwrap().document().unwrap()) }
