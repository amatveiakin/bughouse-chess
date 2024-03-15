use std::ops;

use serde::{Deserialize, Serialize};


// A wrapper for a type (e.g. integer) to allow NaN-like values. Any arithmetic operation involving
// a NaN will result in NaN. Comparisons work differently from float NaNs:
//   - `Nanable` NaNs compare equal to each other;
//   - The is no ordering on `Nanable`s.
//
// Note. It would be more efficient to reserve a special value (e.g. MIN or MAX) to serve as NaN.
// I'm not doing that for the simplicity of the implementation. In particular, note that if the code
// is changed to use a special value as NaN, then `Debug` and `Serialize` should be updated to
// produce something readable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Nanable<T> {
    Regular(T),
    NaN,
}

impl<T> Nanable<T> {
    pub fn is_nan(&self) -> bool { matches!(self, Nanable::NaN) }

    #[track_caller]
    pub fn unwrap(self) -> T {
        match self {
            Nanable::Regular(t) => t,
            Nanable::NaN => panic!("called `Nanable::unwrap()` on a `NaN` value"),
        }
    }
    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Nanable::Regular(t) => t,
            Nanable::NaN => default,
        }
    }
    pub fn get(&self) -> Option<&T> {
        match self {
            Nanable::Regular(t) => Some(t),
            _ => None,
        }
    }
    pub fn into_inner(self) -> Option<T> {
        match self {
            Nanable::Regular(t) => Some(t),
            _ => None,
        }
    }

    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Nanable<U> {
        match self {
            Nanable::Regular(t) => Nanable::Regular(f(t)),
            Nanable::NaN => Nanable::NaN,
        }
    }
    pub fn combine<U, F: FnOnce(T, T) -> U>(self, other: Nanable<T>, f: F) -> Nanable<U> {
        match (self, other) {
            (Nanable::Regular(a), Nanable::Regular(b)) => Nanable::Regular(f(a, b)),
            (Nanable::NaN, _) | (_, Nanable::NaN) => Nanable::NaN,
        }
    }
}

impl<T> Nanable<Option<T>> {
    pub fn transpose(self) -> Option<Nanable<T>> {
        match self {
            Nanable::Regular(Some(t)) => Some(Nanable::Regular(t)),
            Nanable::Regular(None) => None,
            Nanable::NaN => Some(Nanable::NaN),
        }
    }
}

impl<T> From<T> for Nanable<T> {
    fn from(t: T) -> Self { Nanable::Regular(t) }
}

macro_rules! impl_nanable_unary_op {
    ($trait:ident, $method:ident) => {
        impl<T: ops::$trait> ops::$trait for Nanable<T> {
            type Output = Nanable<T::Output>;
            fn $method(self) -> Self::Output {
                match self {
                    Nanable::Regular(a) => Nanable::Regular(a.$method()),
                    Nanable::NaN => Nanable::NaN,
                }
            }
        }
    };
}

macro_rules! impl_nanable_binary_op {
    ($trait:ident, $method:ident) => {
        impl<Rhs, T: ops::$trait<Rhs>> ops::$trait<Nanable<Rhs>> for Nanable<T> {
            type Output = Nanable<T::Output>;
            fn $method(self, rhs: Nanable<Rhs>) -> Self::Output {
                match (self, rhs) {
                    (Nanable::Regular(a), Nanable::Regular(b)) => Nanable::Regular(a.$method(b)),
                    (Nanable::NaN, _) | (_, Nanable::NaN) => Nanable::NaN,
                }
            }
        }
    };
}

macro_rules! impl_nanable_assign_op {
    ($trait:ident, $method:ident) => {
        impl<Rhs, T: ops::$trait<Rhs>> ops::$trait<Nanable<Rhs>> for Nanable<T> {
            fn $method(&mut self, rhs: Nanable<Rhs>) {
                let lhs = std::mem::replace(self, Nanable::NaN);
                *self = match (lhs, rhs) {
                    (Nanable::Regular(mut a), Nanable::Regular(b)) => {
                        a.$method(b);
                        Nanable::Regular(a)
                    }
                    (Nanable::NaN, _) | (_, Nanable::NaN) => Nanable::NaN,
                }
            }
        }
    };
}

impl_nanable_unary_op!(Neg, neg);

impl_nanable_binary_op!(Add, add);
impl_nanable_binary_op!(Sub, sub);
impl_nanable_binary_op!(Mul, mul);
impl_nanable_binary_op!(Div, div);
impl_nanable_binary_op!(Rem, rem);

impl_nanable_assign_op!(AddAssign, add_assign);
impl_nanable_assign_op!(SubAssign, sub_assign);
impl_nanable_assign_op!(MulAssign, mul_assign);
impl_nanable_assign_op!(DivAssign, div_assign);
impl_nanable_assign_op!(RemAssign, rem_assign);


#[cfg(test)]
mod tests {
    use std::time::Instant;

    use instant::Duration;

    use super::*;

    #[test]
    fn methods() {
        let a: Nanable<_> = 42.into();
        assert!(!a.is_nan());
        assert_eq!(*a.get().unwrap(), 42);
        assert_eq!(a.into_inner().unwrap(), 42);
    }

    #[test]
    fn basic_arithmetic() {
        let a = Nanable::Regular(3);
        let b = Nanable::Regular(4);
        let c = Nanable::<i32>::NaN;
        assert_eq!(a + b, Nanable::Regular(7));
        assert_eq!(a + c, Nanable::NaN);
        assert_eq!(c + b, Nanable::NaN);
    }

    #[test]
    fn heterogeneous_arithmetic() {
        let mut t = Nanable::Regular(Instant::now());
        let d = Nanable::Regular(Duration::from_secs(1));
        t += d;
        assert!(!(t + d).is_nan());
    }
}
