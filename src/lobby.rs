use enum_map::{EnumMap, enum_map};

use crate::game::{MIN_PLAYERS, TOTAL_ENVOYS, TOTAL_ENVOYS_PER_TEAM};
use crate::player::{Team, Faction, Participant};
use crate::rules::{Teaming, Rules};


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticipantsError {
    NotEnoughPlayers,
    TooManyPlayersTotal,
    TooManyPlayersInTeam,
    EmptyTeam,
    RatedDoublePlay,
    NotReady,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticipantsWarning {
    NeedToSeatOut,
    NeedToDoublePlay,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParticipantsStatus {
    pub error: Option<ParticipantsError>,
    pub warning: Option<ParticipantsWarning>,
}

impl ParticipantsStatus {
    fn from_error(error: ParticipantsError) -> Self {
        ParticipantsStatus {
            error: Some(error),
            warning: None,
        }
    }
}

pub fn num_fixed_players_per_team<'a>(
    participants: impl Iterator<Item = &'a Participant>
) -> EnumMap<Team, usize> {
    let mut num_players_per_team = enum_map!{ _ => 0 };
    for p in participants {
        if let Faction::Fixed(team) = p.faction {
            num_players_per_team[team] += 1;
        }
    }
    num_players_per_team
}

pub fn verify_participants<'a>(
    rules: &Rules, participants: impl Iterator<Item = &'a Participant> + Clone
) -> ParticipantsStatus {
    // Check total player number.
    let num_players = participants.clone().filter(|p| p.faction.is_player()).count();
    if num_players < MIN_PLAYERS {
        return ParticipantsStatus::from_error(ParticipantsError::NotEnoughPlayers);
    }
    match rules.bughouse_rules.teaming {
        Teaming::FixedTeams => {
            if num_players > TOTAL_ENVOYS {
                return ParticipantsStatus::from_error(ParticipantsError::TooManyPlayersTotal);
            }
        },
        Teaming::IndividualMode => {},
    };

    // Check teams.
    match rules.bughouse_rules.teaming {
        Teaming::FixedTeams => {
            let random_players = participants.clone().filter(|p| p.faction == Faction::Random).count();
            let players_per_team = num_fixed_players_per_team(participants.clone());
            if players_per_team.values().any(|&n| n > TOTAL_ENVOYS_PER_TEAM) {
                return ParticipantsStatus::from_error(ParticipantsError::TooManyPlayersInTeam);
            }
            if players_per_team.values().any(|&n| n == 0) && random_players == 0 {
                return ParticipantsStatus::from_error(ParticipantsError::EmptyTeam);
            }
        }
        Teaming::IndividualMode => {},
    };
    if rules.contest_rules.rated && num_players < TOTAL_ENVOYS {
        return ParticipantsStatus::from_error(ParticipantsError::RatedDoublePlay);
    }

    // Check readiness.
    let error =
        if participants.clone().filter(|p| p.faction.is_player()).all(|p| p.is_ready) {
            None
        } else {
            Some(ParticipantsError::NotReady)
        };

    // Check warnings.
    let warning =
        if num_players < TOTAL_ENVOYS {
            Some(ParticipantsWarning::NeedToDoublePlay)
        } else if num_players > TOTAL_ENVOYS {
            Some(ParticipantsWarning::NeedToSeatOut)
        } else {
            None
        };

    ParticipantsStatus{ error, warning }
}
