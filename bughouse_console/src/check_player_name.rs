use std::io;

use crate::prod_server_helpers::validate_player_name;


pub fn run(user_name: &str) -> io::Result<()> {
    match validate_player_name(user_name) {
        Ok(_) => {
            println!("OK");
            Ok(())
        }
        Err(err) => {
            eprintln!("Invalid player name {}: {}", user_name, err);
            Err(io::Error::from(io::ErrorKind::InvalidData))
        }
    }
}
