// This trait allows to write functions that take a mutable iterator and traverse it multiple times.
// For an immutable iteration this could be achieved by cloning the iterator. However mutable
// iterators cannot be cloned, since this would create multiple mutable references to the same
// object.
//
// Example:
// ```
//   pub fn process<'a>(items: impl Iterator<Item = &'a Foo> + Clone); // ok
//
//   pub fn process_mut<'a>(items: impl Iterator<Item = &'a mut Foo> + Clone); // does not work
//   pub fn process_mut(items: &mut impl IterableMut<Foo>);                    // use this instead
// ```
//
// Note that the first `process_mut` function above compiles fine on its own. However you cannot get
// an iterator to pass to this function, because `iter_mut` returns a non-clonable iterator for all
// standard collections.

// Note. Cannot use names `iter` and `iter_mut`, because this would create conflicts in blanket
// implementations for things like `Vec`.
pub trait IterableMut<T: 'static> {
    fn get_iter<'a>(&'a self) -> impl Iterator<Item = &'a T>;
    fn get_iter_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T>;
}

impl<T: 'static> IterableMut<T> for Vec<T> {
    fn get_iter<'a>(&'a self) -> impl Iterator<Item = &'a T> { self.iter() }
    fn get_iter_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T> { self.iter_mut() }
}
