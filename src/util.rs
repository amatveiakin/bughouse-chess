pub fn sort_two<T: Ord>(v: (T, T)) -> (T, T) {
    let (a, b) = v;
    if a < b { (a, b) } else { (b, a) }
}

// Rust-upgrade (https://github.com/rust-lang/rust/issues/88581):
//   Replace with `a.div_ceil(b)`.
pub fn div_ceil_u128(a: u128, b: u128) -> u128 { (a + b - 1) / b }
