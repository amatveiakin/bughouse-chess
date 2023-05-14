use enum_map::{enum_map, EnumMap};

use crate::game::{MIN_PLAYERS, TOTAL_ENVOYS};
use crate::player::{Faction, Participant, Team};
use crate::rules::{Rules, Teaming};
use crate::TOTAL_ENVOYS_PER_TEAM;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticipantsError {
    NotEnoughPlayers,
    TooManyPlayersTotal,
    EmptyTeam,
    RatedDoublePlay,
    NotReady,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticipantsWarning {
    NeedToDoublePlayAndSeatOut,
    NeedToDoublePlay,
    NeedToSeatOut,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParticipantsStatus {
    pub error: Option<ParticipantsError>,
    pub warning: Option<ParticipantsWarning>,
}

impl ParticipantsStatus {
    fn from_error(error: ParticipantsError) -> Self {
        ParticipantsStatus { error: Some(error), warning: None }
    }
}

pub fn num_fixed_players_per_team<'a>(
    participants: impl Iterator<Item = &'a Participant>,
) -> EnumMap<Team, usize> {
    let mut num_players_per_team = enum_map! { _ => 0 };
    for p in participants {
        if let Faction::Fixed(team) = p.faction {
            num_players_per_team[team] += 1;
        }
    }
    num_players_per_team
}

pub fn verify_participants<'a>(
    rules: &Rules, participants: impl Iterator<Item = &'a Participant> + Clone,
) -> ParticipantsStatus {
    let total_players = participants.clone().filter(|p| p.faction.is_player()).count();
    if total_players < MIN_PLAYERS {
        return ParticipantsStatus::from_error(ParticipantsError::NotEnoughPlayers);
    }

    let mut need_to_double_play = total_players < TOTAL_ENVOYS;
    let mut need_to_seat_out = total_players > TOTAL_ENVOYS;
    match rules.bughouse_rules.teaming {
        Teaming::FixedTeams => {
            let random_players =
                participants.clone().filter(|p| p.faction == Faction::Random).count();
            let players_per_team = num_fixed_players_per_team(participants.clone());
            for &team_players in players_per_team.values() {
                if team_players + random_players == 0 {
                    return ParticipantsStatus::from_error(ParticipantsError::EmptyTeam);
                } else if team_players + random_players < TOTAL_ENVOYS_PER_TEAM {
                    // Note. This test relies on the fact that we have exactly two teams and that
                    // we've already checked total player number.
                    need_to_double_play = true;
                } else if team_players > TOTAL_ENVOYS_PER_TEAM {
                    need_to_seat_out = true;
                }
            }
        }
        Teaming::IndividualMode => {}
    };

    if rules.match_rules.rated && need_to_double_play {
        return ParticipantsStatus::from_error(ParticipantsError::RatedDoublePlay);
    }

    let players_ready = participants.filter(|p| p.faction.is_player()).all(|p| p.is_ready);
    let error = if players_ready {
        None
    } else {
        Some(ParticipantsError::NotReady)
    };

    let warning = match (need_to_double_play, need_to_seat_out) {
        (true, true) => Some(ParticipantsWarning::NeedToDoublePlayAndSeatOut),
        (true, false) => Some(ParticipantsWarning::NeedToDoublePlay),
        (false, true) => Some(ParticipantsWarning::NeedToSeatOut),
        (false, false) => None,
    };

    ParticipantsStatus { error, warning }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{BughouseRules, ChessRules, MatchRules};

    fn make_rules(teaming: Teaming, rated: bool) -> Rules {
        Rules {
            chess_rules: ChessRules::classic_blitz(),
            bughouse_rules: BughouseRules { teaming, ..BughouseRules::chess_com() },
            match_rules: MatchRules { rated },
        }
    }

    fn make_participant(faction: Faction, is_ready: bool) -> Participant {
        Participant {
            name: "player".to_owned(),
            is_registered_user: false,
            faction,
            games_played: 0,
            is_online: true,
            is_ready,
        }
    }

    #[test]
    fn verify_participants_test() {
        assert_eq!(
            verify_participants(
                &make_rules(Teaming::IndividualMode, true),
                [
                    make_participant(Faction::Random, false),
                    make_participant(Faction::Random, false),
                    make_participant(Faction::Random, false),
                ]
                .iter()
            ),
            ParticipantsStatus {
                error: Some(ParticipantsError::RatedDoublePlay),
                warning: None
            }
        );

        assert_eq!(
            verify_participants(
                &make_rules(Teaming::IndividualMode, false),
                [
                    make_participant(Faction::Random, false),
                    make_participant(Faction::Random, false),
                    make_participant(Faction::Random, false),
                ]
                .iter()
            ),
            ParticipantsStatus {
                error: Some(ParticipantsError::NotReady),
                warning: Some(ParticipantsWarning::NeedToDoublePlay),
            }
        );

        assert_eq!(
            verify_participants(
                &make_rules(Teaming::FixedTeams, false),
                [
                    make_participant(Faction::Fixed(Team::Red), true),
                    make_participant(Faction::Fixed(Team::Blue), true),
                    make_participant(Faction::Fixed(Team::Blue), true),
                    make_participant(Faction::Fixed(Team::Blue), true),
                ]
                .iter()
            ),
            ParticipantsStatus {
                error: None,
                warning: Some(ParticipantsWarning::NeedToDoublePlayAndSeatOut),
            }
        );

        assert_eq!(
            verify_participants(
                &make_rules(Teaming::FixedTeams, false),
                [
                    make_participant(Faction::Fixed(Team::Red), true),
                    make_participant(Faction::Fixed(Team::Blue), true),
                    make_participant(Faction::Fixed(Team::Blue), true),
                    make_participant(Faction::Random, true),
                ]
                .iter()
            ),
            ParticipantsStatus { error: None, warning: None }
        );
    }
}
