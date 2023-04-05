// Server functionality which:
//   - Is not important for testing;
//   - Can potentially blow up WASM size, e.g. due to new Carge dependencies. Rust linker
//     seems to do a good job filtering these out, but it still feels safer. And improves
//     client build time.
pub trait ServerHelpers {
    fn validate_player_name(&self, name: &str) -> Result<(), String>;
}

pub struct TestServerHelpers;

impl ServerHelpers for TestServerHelpers {
    fn validate_player_name(&self, _name: &str) -> Result<(), String> { Ok(()) }
}
