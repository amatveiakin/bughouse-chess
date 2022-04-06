pub fn sort_two<T: Ord>(v: (T, T)) -> (T, T) {
    let (a, b) = v;
    if a < b { (a, b) } else { (b, a) }
}
