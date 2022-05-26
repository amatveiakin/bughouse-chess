use enum_map::{EnumArray, EnumMap, enum_map};


pub fn sort_two<T: Ord>(v: (T, T)) -> (T, T) {
    let (a, b) = v;
    if a < b { (a, b) } else { (b, a) }
}

// Rust-upgrade (https://github.com/rust-lang/rust/issues/88581):
//   Replace with `a.div_ceil(b)`.
pub fn div_ceil_u128(a: u128, b: u128) -> u128 { (a + b - 1) / b }

// Improvement potential: Implement Serde support for EnumMap instead.
pub fn try_vec_to_enum_map<K, V>(vec: Vec<(K, V)>) -> Option<EnumMap<K, V>>
where
    K: EnumArray<V> + EnumArray<Option<V>> + Copy,
{
    let mut map: EnumMap<K, Option<V>> = enum_map!{ _ => None };
    if vec.len() != map.len() {
        return None;
    }
    for (key, value) in vec.into_iter() {
        if map[key].is_some() {
            return None;
        }
        map[key] = Some(value);
    }
    Some(map.map(|_, v| v.unwrap()))
}
