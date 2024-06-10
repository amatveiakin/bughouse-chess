macro_rules! impl_collection_iterator {
    ($iterator:ident, $into_iterator:ident, $collection:ty, $item:ty) => {
        pub struct $iterator {
            collection: $collection,
            index: u32,
        }

        pub trait $into_iterator {
            #[allow(dead_code)]
            fn into_iterator(self) -> $iterator;
        }

        impl From<$collection> for $iterator {
            fn from(collection: $collection) -> Self { Self { collection, index: 0 } }
        }

        impl $into_iterator for $collection {
            fn into_iterator(self) -> $iterator { self.into() }
        }

        impl Iterator for $iterator {
            type Item = $item;

            fn next(&mut self) -> Option<Self::Item> {
                let item = self.collection.item(self.index);
                self.index += 1;
                item
            }
        }
    };
}

impl_collection_iterator!(
    HtmlCollectionIterator,
    IntoHtmlCollectionIterator,
    web_sys::HtmlCollection,
    web_sys::Element
);
impl_collection_iterator!(NodeListIterator, IntoNodeListIterator, web_sys::NodeList, web_sys::Node);
