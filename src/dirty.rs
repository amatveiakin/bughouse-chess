use std::cell::Cell;
use std::ops;


#[derive(Clone, Debug)]
pub struct Dirty<T> {
    value: T,
    dirty: Cell<bool>,
}

impl<T> Dirty<T> {
    pub fn new(value: T) -> Self { Self { value, dirty: Cell::new(false) } }

    pub fn get_mut(&mut self) -> &mut T {
        self.dirty.set(true);
        &mut self.value
    }

    pub fn take_dirt(&self) -> bool { self.dirty.replace(false) }
}

impl<T: Eq> Dirty<T> {
    pub fn set(&mut self, value: T) {
        if self.value != value {
            self.value = value;
            self.dirty.set(true);
        }
    }
}

impl<T> ops::Deref for Dirty<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.value }
}
// Don't implement `DerefMut`. A call to `get_mut` stresses the fact that it sets the dirty flag.
