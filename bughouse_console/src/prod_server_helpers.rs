use bughouse_chess::server_helpers::ServerHelpers;
use censor::Censor;

use crate::censor::profanity_censor;


#[allow(clippy::useless_format)]
pub fn validate_player_name(name: &str) -> Result<(), String> {
    const MIN_NAME_LENGTH: usize = 2;
    const MAX_NAME_LENGTH: usize = 20;

    // These words cannot be used inside player names, even with slight variations.
    const CUSTOM_CENSOR: &[&'static str] = &["admin", "guest"];

    // These words cannot be used as player names to avoid confusion in system messages.
    // They can be used inside player names, though.
    #[rustfmt::skip]
    const CUSTOM_BAN: &[&'static str] = &[
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
    let len = name.chars().count();
    if len < MIN_NAME_LENGTH {
        return Err(format!("Minimum name length is {MIN_NAME_LENGTH}."));
    }
    if len > MAX_NAME_LENGTH {
        return Err(format!("Maximum name length is {MAX_NAME_LENGTH}."));
    }
    if profanity_censor().check(&name)
        || Censor::custom(CUSTOM_CENSOR.iter().copied()).check(&name)
        || contains_ignoring_ascii_case(CUSTOM_BAN, &name)
    {
        return Err(format!("Please try another player name."));
    }
    Ok(())
}

fn contains_ignoring_ascii_case(haystack: &[&str], needle: &str) -> bool {
    haystack.iter().any(|&s| s.eq_ignore_ascii_case(needle))
}


pub struct ProdServerHelpers;

impl ServerHelpers for ProdServerHelpers {
    // Validates player name. Simple tests (such as length and character set) are also done
    // on the client.
    fn validate_player_name(&self, name: &str) -> Result<(), String> { validate_player_name(name) }
}
