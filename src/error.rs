#[macro_export]
macro_rules! internal_error_message {
    () => {
        format!("Internal error at {}:{}.", file!(), line!())
    };
    ($($arg:tt)+) => {
        format!("Internal error at {}:{}: {}.", file!(), line!(), format!($($arg)*))
    };
}
