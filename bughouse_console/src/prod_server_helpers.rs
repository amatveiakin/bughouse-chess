use bughouse_chess::server_helpers::ServerHelpers;
use censor::Censor;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::censor::profanity_censor;


#[allow(clippy::useless_format)]
pub fn validate_player_name(name: &str) -> Result<(), String> {
    let name = name.nfc().collect::<String>();
    const MIN_NAME_LENGTH: usize = 2;
    const MAX_NAME_LENGTH: usize = 16;
    if !name.chars().all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_') {
        return Err(format!(
            "Player name must consist of letters, digits, dashes ('-') and underscores ('_')."
        ));
    }
    if !name.chars().any(|ch| ch.is_alphabetic()) {
        // Requiring that the name contains a letter reduces the risk of collision if
        // e.g. we decide to have a DB column that stores either guest name or registered
        // user ID. Also it just makes sense.
        return Err(format!("Player name must contain at least one letter."));
    }
    let len = name.graphemes(true).count();
    if len < MIN_NAME_LENGTH {
        return Err(format!("Minimum name length is {MIN_NAME_LENGTH}."));
    }
    if len > MAX_NAME_LENGTH {
        return Err(format!("Maximum name length is {MAX_NAME_LENGTH}."));
    }
    if profanity_censor().check(&name) {
        return Err(format!("Player name must not contain profanity."));
    }
    if Censor::custom(["admin", "guest"]).check(&name) {
        return Err(format!(r#"Player name must not contain words "admin" or "guest"."#));
    }
    Ok(())
}

pub struct ProdServerHelpers;

impl ServerHelpers for ProdServerHelpers {
    // Validates player name. Simple tests (such as length and character set) are also done
    // on the client.
    // TODO: Also convert to NFC and count graphemes in the web client.
    fn validate_player_name(&self, name: &str) -> Result<(), String> { validate_player_name(name) }
}
