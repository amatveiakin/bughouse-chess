use std::cmp;
use std::collections::BTreeMap;

use enum_map::{enum_map, EnumMap};

use crate::game::{MIN_PLAYERS, TOTAL_ENVOYS, TOTAL_ENVOYS_PER_TEAM, TOTAL_TEAMS};
use crate::player::{Faction, Participant, Team};
use crate::rules::Rules;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Teaming {
    FixedTeams,
    DynamicTeams,
}

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

    let random_players = participants.clone().filter(|p| p.faction == Faction::Random).count();
    let players_per_team = num_fixed_players_per_team(participants.clone());
    let mut need_to_double_play = total_players < TOTAL_ENVOYS;
    let mut need_to_seat_out = total_players > TOTAL_ENVOYS;
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

// If teams are bound to be the same every game, sets a fixed team for every participant with
// Faction::Random and returns Teaming::FixedTeams. Otherwise, returns Teaming::DynamicTeams.
//
// Assumes `verify_participants` returns no error.
//
// TODO: Try to accept something like `participants: impl Iterator<Item = &'a mut Participant>`
// instead of concrete container type. The problem is: Rust iterators are not rewindable and mutable
// iterators are not clonable.
pub fn fix_teams_if_needed<T>(participants: &mut BTreeMap<T, Participant>) -> Teaming {
    let total_players = participants.len();
    let random_players = participants.values().filter(|p| p.faction == Faction::Random).count();
    if random_players == 0 {
        return Teaming::FixedTeams;
    }
    let players_per_team = num_fixed_players_per_team(participants.values());

    // Teams are always the same iff all random players must go into the same team.
    let mut random_players_team = None;

    for (team, &team_players) in players_per_team.iter() {
        let max_expected_players =
            cmp::min(total_players.div_ceil(TOTAL_TEAMS), TOTAL_ENVOYS_PER_TEAM);
        if team_players < max_expected_players {
            if let Some(random_players_team) = random_players_team {
                if random_players_team != team {
                    return Teaming::DynamicTeams;
                }
            } else {
                random_players_team = Some(team);
            }
        }
    }

    if let Some(random_players_team) = random_players_team {
        for p in participants.values_mut() {
            if p.faction == Faction::Random {
                p.faction = Faction::Fixed(random_players_team);
            }
        }
        Teaming::FixedTeams
    } else {
        Teaming::DynamicTeams
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{BughouseRules, ChessRules, MatchRules};

    fn make_rules(rated: bool) -> Rules {
        Rules {
            chess_rules: ChessRules::classic_blitz(),
            bughouse_rules: BughouseRules::chess_com(),
            match_rules: MatchRules { rated },
        }
    }

    fn add_participant(
        participants: &mut BTreeMap<String, Participant>, name: &str, faction: Faction,
        is_ready: bool,
    ) {
        participants.insert(name.to_owned(), Participant {
            name: name.to_owned(),
            is_registered_user: false,
            faction,
            games_played: 0,
            is_online: true,
            is_ready,
        });
    }

    #[test]
    fn three_random_players() {
        let mut participants = BTreeMap::new();
        add_participant(&mut participants, "p1", Faction::Random, false);
        add_participant(&mut participants, "p2", Faction::Random, false);
        add_participant(&mut participants, "p3", Faction::Random, false);
        assert_eq!(
            verify_participants(&make_rules(true), participants.values()),
            ParticipantsStatus {
                error: Some(ParticipantsError::RatedDoublePlay),
                warning: None
            }
        );
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus {
                error: Some(ParticipantsError::NotReady),
                warning: Some(ParticipantsWarning::NeedToDoublePlay),
            }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::DynamicTeams);
        assert!(participants.values().all(|p| p.faction == Faction::Random));
    }

    #[test]
    fn three_vs_one() {
        let mut participants = BTreeMap::new();
        add_participant(&mut participants, "p1", Faction::Fixed(Team::Red), true);
        add_participant(&mut participants, "p2", Faction::Fixed(Team::Blue), true);
        add_participant(&mut participants, "p3", Faction::Fixed(Team::Blue), true);
        add_participant(&mut participants, "p4", Faction::Fixed(Team::Blue), true);
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus {
                error: None,
                warning: Some(ParticipantsWarning::NeedToDoublePlayAndSeatOut),
            }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
    }

    #[test]
    fn two_players_fixable() {
        let mut participants = BTreeMap::new();
        add_participant(&mut participants, "p1", Faction::Fixed(Team::Red), true);
        add_participant(&mut participants, "p2", Faction::Random, true);
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
        assert_eq!(participants["p1"].faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p2"].faction, Faction::Fixed(Team::Blue));
    }

    #[test]
    fn three_players_fixable() {
        let mut participants = BTreeMap::new();
        add_participant(&mut participants, "p1", Faction::Fixed(Team::Red), true);
        add_participant(&mut participants, "p2", Faction::Fixed(Team::Red), true);
        add_participant(&mut participants, "p3", Faction::Random, true);
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
        assert_eq!(participants["p1"].faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p2"].faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p3"].faction, Faction::Fixed(Team::Blue));
    }

    #[test]
    fn three_players_unfixable() {
        let mut participants = BTreeMap::new();
        add_participant(&mut participants, "p1", Faction::Fixed(Team::Red), true);
        add_participant(&mut participants, "p2", Faction::Random, true);
        add_participant(&mut participants, "p3", Faction::Random, true);
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::DynamicTeams);
    }

    #[test]
    fn four_players_unfixable() {
        let mut participants = BTreeMap::new();
        add_participant(&mut participants, "p1", Faction::Fixed(Team::Red), true);
        add_participant(&mut participants, "p2", Faction::Fixed(Team::Blue), true);
        add_participant(&mut participants, "p3", Faction::Random, true);
        add_participant(&mut participants, "p4", Faction::Random, true);
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus { error: None, warning: None }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::DynamicTeams);
    }

    #[test]
    fn four_players_fixable() {
        let mut participants = BTreeMap::new();
        add_participant(&mut participants, "p1", Faction::Fixed(Team::Red), true);
        add_participant(&mut participants, "p2", Faction::Fixed(Team::Blue), true);
        add_participant(&mut participants, "p3", Faction::Fixed(Team::Blue), true);
        add_participant(&mut participants, "p4", Faction::Random, true);
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus { error: None, warning: None }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
        assert_eq!(participants["p1"].faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p2"].faction, Faction::Fixed(Team::Blue));
        assert_eq!(participants["p3"].faction, Faction::Fixed(Team::Blue));
        assert_eq!(participants["p4"].faction, Faction::Fixed(Team::Red));
    }
}
