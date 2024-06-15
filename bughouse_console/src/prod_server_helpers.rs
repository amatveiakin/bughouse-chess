use bughouse_chess::server_helpers::ServerHelpers;
use censor::Censor;
use itertools::Itertools;

use crate::censor::profanity_censor;


#[allow(clippy::useless_format)]
pub fn validate_player_name(name: &str) -> Result<(), String> {
    const MIN_NAME_LENGTH: usize = 2;
    const MAX_NAME_LENGTH: usize = 20;

    // These words cannot be used inside player names, even with slight variations.
    const CUSTOM_CENSOR: &[&str] = &["admin", "guest"];

    // These words cannot be used as player names to avoid confusion in system messages.
    // They can be used inside player names, though.
    #[rustfmt::skip]
    const CUSTOM_BAN: &[&str] = &[
        // Pronouns
        "I", "me", "myself", "mine", "my",
        "we", "us", "ourselves", "ourself", "ours", "our",
        "you", "yourselves", "yourself", "yours", "your",
        "he", "him", "himself", "his",
        "she", "her", "herself", "hers",
        "it", "itself", "its",
        "they", "them", "themselves", "themself", "theirs", "their",
        "one", "oneself",
        "all", "another", "any", "anybody", "anyone", "anything",
        "both", "each", "either", "everybody", "everyone", "everything",
        "few", "many", "most", "neither", "nobody", "none", "nothing",
        "other", "others",
        "several", "some", "somebody", "someone", "something", "such",
        "what", "whatever", "which", "whichever", "who", "whoever", "whom", "whomever", "whose",
        "as", "that",
        // Common prepositions
        "and", "as", "at", "by", "for", "from", "if", "in", "like", "of", "off", "on", "or",
        "than", "then", "to", "via", "versus", "vs", "with",
        // Directions
        "up", "down", "left", "right", "top", "bottom", "front", "back", "forward", "backward",
        // Chess terms
        "chess", "bughouse",
        "board", "piece", "turn", "move",
        "check", "mate", "stalemate", "resign", "resigned",
        "win", "won", "victory", "lost", "loss", "defeat", "draw", "drew", "tie", "tied",
        "participant", "player", "observer", "spectator", "watcher",
        "white", "black",
        "pawn", "knight", "bishop", "rook", "queen", "king", "duck",
    ];

    if !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        return Err(format!(
            "Player name may consist of Latin letters, digits, dashes ('-') and underscores ('_')."
        ));
    }
    if !name.chars().any(|ch| ch.is_ascii_alphabetic()) {
        // Requiring that the name contains a letter reduces the risk of collision if
        // e.g. we decide to have a DB column that stores either guest name or registered
        // user ID. Also it just makes sense.
        return Err(format!("Player name must contain at least one letter."));
    }
    if !name.chars().tuple_windows().all(valid_consecutive_letters) {
        return Err(format!("Player name cannot contain several punctuation marks in a row."));
    }
    if !name.chars().next().unwrap().is_ascii_alphanumeric() {
        return Err(format!("Player name must start with a letter or number."));
    }
    if !name.chars().last().unwrap().is_ascii_alphanumeric() {
        return Err(format!("Player name must end with a letter or number."));
    }
    let len = name.chars().count();
    if len < MIN_NAME_LENGTH {
        return Err(format!("Minimum name length is {MIN_NAME_LENGTH}."));
    }
    if len > MAX_NAME_LENGTH {
        return Err(format!("Maximum name length is {MAX_NAME_LENGTH}."));
    }
    if profanity_censor().check(name)
        || Censor::custom(CUSTOM_CENSOR.iter().copied()).check(name)
        || contains_ignoring_ascii_case(CUSTOM_BAN, name)
    {
        return Err(format!("Please try another player name."));
    }
    Ok(())
}

fn contains_ignoring_ascii_case(haystack: &[&str], needle: &str) -> bool {
    haystack.iter().any(|&s| s.eq_ignore_ascii_case(needle))
}

fn valid_consecutive_letters((a, b): (char, char)) -> bool {
    a.is_ascii_alphanumeric() || b.is_ascii_alphanumeric()
}


pub struct ProdServerHelpers;

impl ServerHelpers for ProdServerHelpers {
    // Validates player name. Simple tests (such as length and character set) are also done
    // on the client.
    fn validate_player_name(&self, name: &str) -> Result<(), String> { validate_player_name(name) }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_player_name_all() {
        validate_player_name("a").unwrap_err(); // too short
        validate_player_name("123456789o123456789o").unwrap();
        validate_player_name("123456789o123456789o1").unwrap_err(); // too long

        validate_player_name("ab").unwrap();
        validate_player_name("12").unwrap_err(); // no letters
        validate_player_name("a1").unwrap();
        validate_player_name("a#").unwrap_err(); // illegal characters
        validate_player_name("игрок").unwrap_err(); // illegal characters

        validate_player_name("some").unwrap_err(); // reserved word
        validate_player_name("some_player").unwrap(); // reserved word inside
        validate_player_name("admin").unwrap_err(); // banned word
        validate_player_name("MainAdmin").unwrap_err(); // banned word inside

        validate_player_name("Ok-c_oo_l-name").unwrap(); // special characters OK
        validate_player_name("too-_-cool").unwrap_err(); // consecutive special characters
        validate_player_name("still__bad").unwrap_err(); // trailing special characters
        validate_player_name("_bad").unwrap_err(); // trailing special characters
        validate_player_name("bad_").unwrap_err(); // trailing special characters
    }
}
