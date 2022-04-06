use std::fmt;
use std::ops::{Deref, DerefMut};


pub struct Janitor<T, F>
where
    T: DerefMut,
    F: for<'a> Fn(&'a mut T::Target),
{
    value: T,
    on_scope_end: F,
}

impl <T, F> Janitor<T, F>
where
    T: DerefMut,
    F: for<'a> Fn(&'a mut T::Target),
{
    pub fn new(value: T, on_scope_end: F) -> Self {
        Self { value, on_scope_end }
    }
}

impl <T, F> fmt::Debug for Janitor<T, F>
where
    T: DerefMut + fmt::Debug,
    F: for<'a> Fn(&'a mut T::Target),
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Janitor")
            .field(&self.value)
            .finish()
    }
}

impl <T, F> Deref for Janitor<T, F>
where
    T: DerefMut,
    F: for<'a> Fn(&'a mut T::Target),
{
    type Target = T::Target;

    fn deref(&self) -> &Self::Target {
        self.value.deref()
    }
}

impl <T, F> DerefMut for Janitor<T, F>
where
    T: DerefMut,
    F: for<'a> Fn(&'a mut T::Target),
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.deref_mut()
    }
}

impl <T, F> Drop for Janitor<T, F>
where
    T: DerefMut,
    F: for<'a> Fn(&'a mut T::Target),
{
    fn drop(&mut self) {
        (self.on_scope_end)(self.value.deref_mut());
    }
}
