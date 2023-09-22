pub struct HtmlCollectionIterator {
    collection: web_sys::HtmlCollection,
    index: u32,
}

// Cannot implement `IntoIterator` for `HtmlCollection` because both the trait and the struct are
// foreign.
pub trait IntoHtmlCollectionIterator {
    fn into_iterator(self) -> HtmlCollectionIterator;
}

impl From<web_sys::HtmlCollection> for HtmlCollectionIterator {
    fn from(collection: web_sys::HtmlCollection) -> Self { Self { collection, index: 0 } }
}

impl IntoHtmlCollectionIterator for web_sys::HtmlCollection {
    fn into_iterator(self) -> HtmlCollectionIterator { self.into() }
}

impl Iterator for HtmlCollectionIterator {
    type Item = web_sys::Element;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.collection.item(self.index);
        self.index += 1;
        item
    }
}
