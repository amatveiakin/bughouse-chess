pub use git_version::git_version;
use itertools::Itertools;


#[macro_export]
macro_rules! my_git_version {
    () => {
        // TODO: Fix missing git version in Docker.
        $crate::util::git_version!(args = ["--tags", "--always", "--dirty=-modified"], fallback = "unknown")
    };
}

// Slightly adjusted macro from https://docs.rs/once_cell/latest/once_cell/#lazily-compiled-regex:
#[macro_export]
macro_rules! once_cell_regex {
    ($re:expr $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

pub fn sort_two<T: Ord>((a, b): (T, T)) -> (T, T) {
    if a < b { (a, b) } else { (b, a) }
}
pub fn sort_two_desc<T: Ord>((a, b): (T, T)) -> (T, T) {
    if a > b { (a, b) } else { (b, a) }
}

// Rust-upgrade (https://github.com/rust-lang/rust/issues/88581):
//   Replace with `a.div_ceil(b)`.
pub fn div_ceil_u128(a: u128, b: u128) -> u128 { (a + b - 1) / b }

// If a string consists of a single character, returns the character. Otherwise returns none.
pub fn as_single_char(s: &str) -> Option<char> {
    s.chars().collect_tuple().map(|(single_char,)| single_char)
}
