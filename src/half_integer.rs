use std::ops;

use serde::{Deserialize, Serialize};


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HalfU32 {
    doubled: u32,
}

impl HalfU32 {
    pub const ZERO: Self = Self { doubled: 0 };
    pub const HALF: Self = Self { doubled: 1 };

    pub fn whole(value: u32) -> Self { Self { doubled: value * 2 } }

    pub fn as_f64(&self) -> f64 { self.doubled as f64 / 2.0 }
}

impl ops::Add for HalfU32 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output { Self { doubled: self.doubled + rhs.doubled } }
}
impl ops::Sub for HalfU32 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output { Self { doubled: self.doubled - rhs.doubled } }
}

impl ops::AddAssign for HalfU32 {
    fn add_assign(&mut self, rhs: Self) { *self = *self + rhs; }
}
impl ops::SubAssign for HalfU32 {
    fn sub_assign(&mut self, rhs: Self) { *self = *self - rhs; }
}
