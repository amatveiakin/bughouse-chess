#[macro_export]
macro_rules! internal_error_message {
    () => {
        format!("Internal error at {}:{} in {}.", file!(), line!(), $crate::my_git_version!())
    };
    ($($arg:tt)+) => {
        format!(
            "Internal error at {}:{} in {}: {}.",
            file!(), line!(), $crate::my_git_version!(), format!($($arg)*))
    };
}
