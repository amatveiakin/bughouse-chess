pub struct HtmlCollectionIterator {
    collection: web_sys::HtmlCollection,
    index: u32,
}

impl From<web_sys::HtmlCollection> for HtmlCollectionIterator {
    fn from(collection: web_sys::HtmlCollection) -> Self { Self { collection, index: 0 } }
}

impl Iterator for HtmlCollectionIterator {
    type Item = web_sys::Element;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.collection.item(self.index);
        self.index += 1;
        item
    }
}
