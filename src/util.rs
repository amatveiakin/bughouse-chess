pub use git_version::git_version;
use itertools::Itertools;


#[macro_export]
macro_rules! my_git_version {
    () => {
        // TODO: Fix missing git version in Docker.
        $crate::util::git_version!(
            args = ["--tags", "--always", "--dirty=-modified"],
            fallback = "unknown"
        )
    };
}

// Slightly adjusted macro from https://docs.rs/once_cell/latest/once_cell/#lazily-compiled-regex:
#[macro_export]
macro_rules! once_cell_regex {
    ($re:expr $(,)?) => {{
        static RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| regex_lite::Regex::new($re).unwrap())
    }};
}

pub trait Relax {
    fn relax_min(&mut self, other: Self);
    fn relax_max(&mut self, other: Self);
}

impl<T: Ord> Relax for T {
    fn relax_min(&mut self, other: Self) {
        if other < *self {
            *self = other;
        }
    }

    fn relax_max(&mut self, other: Self) {
        if other > *self {
            *self = other;
        }
    }
}

pub fn sort_two<T: Ord>((a, b): (T, T)) -> (T, T) { if a < b { (a, b) } else { (b, a) } }
pub fn sort_two_desc<T: Ord>((a, b): (T, T)) -> (T, T) { if a > b { (a, b) } else { (b, a) } }

// If a string consists of a single character, returns the character. Otherwise returns none.
pub fn as_single_char(s: &str) -> Option<char> {
    s.chars().collect_tuple().map(|(single_char,)| single_char)
}
