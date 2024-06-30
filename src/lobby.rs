// TODO: Rename this module.

use std::collections::HashMap;
use std::{cmp, mem};

use enum_map::{enum_map, EnumMap};
use itertools::Itertools;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::game::{
    get_bughouse_force, BughouseBoard, BughouseEnvoy, BughousePlayer, PlayerInGame, MIN_PLAYERS,
    TOTAL_ENVOYS, TOTAL_ENVOYS_PER_TEAM, TOTAL_TEAMS,
};
use crate::iterable_mut::IterableMut;
use crate::player::{Faction, Participant, Team};
use crate::rules::Rules;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Teaming {
    FixedTeams,
    DynamicTeams,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ParticipantsError {
    NotEnoughPlayers,
    EmptyTeam,
    RatedDoublePlay,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticipantsWarning {
    NeedToDoublePlayAndSeatOut,
    NeedToDoublePlay,
    NeedToSeatOut,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticipantsStatus {
    CanStart {
        players_ready: bool,
        warning: Option<ParticipantsWarning>,
    },
    CannotStart(ParticipantsError),
}

impl ParticipantsStatus {
    pub fn can_start_when_ready(&self) -> bool {
        matches!(self, ParticipantsStatus::CanStart { .. })
    }
    pub fn can_start_now(&self) -> bool {
        matches!(self, ParticipantsStatus::CanStart { players_ready: true, .. })
    }
}

pub fn num_fixed_players_per_team<'a>(
    participants: impl Iterator<Item = &'a Participant>,
) -> EnumMap<Team, usize> {
    let mut num_players_per_team = enum_map! { _ => 0 };
    for p in participants {
        if let Faction::Fixed(team) = p.active_faction {
            num_players_per_team[team] += 1;
        }
    }
    num_players_per_team
}

pub fn verify_participants<'a>(
    rules: &Rules, participants: impl Iterator<Item = &'a Participant> + Clone,
) -> ParticipantsStatus {
    let total_players = participants.clone().filter(|p| p.active_faction.is_player()).count();
    if total_players < MIN_PLAYERS {
        return ParticipantsStatus::CannotStart(ParticipantsError::NotEnoughPlayers);
    }

    let random_players =
        participants.clone().filter(|p| p.active_faction == Faction::Random).count();
    let players_per_team = num_fixed_players_per_team(participants.clone());
    let mut need_to_double_play = total_players < TOTAL_ENVOYS;
    let mut need_to_seat_out = total_players > TOTAL_ENVOYS;
    for &team_players in players_per_team.values() {
        if team_players + random_players == 0 {
            return ParticipantsStatus::CannotStart(ParticipantsError::EmptyTeam);
        } else if team_players + random_players < TOTAL_ENVOYS_PER_TEAM {
            // Note. This test relies on the fact that we have exactly two teams and that
            // we've already checked total player number.
            need_to_double_play = true;
        } else if team_players > TOTAL_ENVOYS_PER_TEAM {
            need_to_seat_out = true;
        }
    }

    if rules.match_rules.rated && need_to_double_play {
        return ParticipantsStatus::CannotStart(ParticipantsError::RatedDoublePlay);
    }

    let players_ready = participants.filter(|p| p.active_faction.is_player()).all(|p| p.is_ready);
    let warning = match (need_to_double_play, need_to_seat_out) {
        (true, true) => Some(ParticipantsWarning::NeedToDoublePlayAndSeatOut),
        (true, false) => Some(ParticipantsWarning::NeedToDoublePlay),
        (false, true) => Some(ParticipantsWarning::NeedToSeatOut),
        (false, false) => None,
    };
    ParticipantsStatus::CanStart { players_ready, warning }
}

// If teams are bound to be the same every game, sets a fixed team for every participant with
// Faction::Random and returns Teaming::FixedTeams. Otherwise, returns Teaming::DynamicTeams.
//
// Assumes `verify_participants` returns no error.
pub fn fix_teams_if_needed(participants: &mut impl IterableMut<Participant>) -> Teaming {
    let total_players = participants.get_iter().count();
    let random_players =
        participants.get_iter().filter(|p| p.active_faction == Faction::Random).count();
    if random_players == 0 {
        return Teaming::FixedTeams;
    }
    let players_per_team = num_fixed_players_per_team(participants.get_iter());

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
        for p in participants.get_iter_mut() {
            if p.active_faction == Faction::Random {
                let new_faction = Faction::Fixed(random_players_team);
                p.active_faction = new_faction;
                p.desired_faction = new_faction;
            }
        }
        Teaming::FixedTeams
    } else {
        Teaming::DynamicTeams
    }
}

// Assigns boards to players. Also assigns teams to players without a fixed team.
//
// Priorities (from highest to lowest):
//   1. Don't make people double play if they don't have to.
//   2. Keep stable assignments when players join active match. After next game players are
//      announced publicly, we want to stick to the assignment.
//   3. Balance the number of games played by each person. [*]
//   4. Balance the number of times people double play (if they have to).
//   5. Uniformly randomize teams, opponents and seating out order.
//
// [*] Actually, we balance the number of games missed. Game missed is defined as a game when a
//     player was ready to play, but had to seat out.
//
// Improvement potential: With the current scheme if a player decides to seat out one game in the
//   middle of a long match they will permanently have fewer games played and thus lower expected
//   score. Would be nice to fix this. A radical solution is to balance by games played rather than
//   games missed, but this has a different downside: with this approach, if a player joins in the
//   middle of the match they are assigned to play many games in a row and other players will seat
//   out way more often, which doesn't seem fair. So what we really seem to want is to primarily
//   balance by games missied, but slightly skew the distribution to balance out games played.
pub fn assign_boards<'a>(
    participants: impl Iterator<Item = &'a Participant> + Clone,
    current_assignment: Option<&[PlayerInGame]>, rng: &mut impl Rng,
) -> Vec<PlayerInGame> {
    let current_assignment = current_assignment
        .map(|current| current.iter().map(|p| (p.name.clone(), p.id)).collect::<HashMap<_, _>>());
    let current_assignment = current_assignment.as_ref();

    let players = participants
        .filter(|p| p.active_faction.is_player())
        .map(|p| {
            let high_priority =
                current_assignment.map_or(false, |current| current.contains_key(&p.name));
            let priority_key = (cmp::Reverse(high_priority), cmp::Reverse(p.games_missed));
            (p, priority_key)
        })
        .collect_vec();
    let priority_buckets =
        players.into_iter().sorted_by_key(|(_, key)| *key).group_by(|(_, key)| *key);

    // Note. Even though we randomize the team for players with `Faction::Random`, randomizing
    // the order within each bucket is still necessary for multiple reasons:
    //   - To randomize opponents;
    //   - To randomize seating out;
    //   - To make team randomization uniform: if we iterated the array [p1, p2, p3, p4] in the
    //     same order and assigned each player to a random non-full team with equal probability,
    //     then p1 and p2 would be on the same team with probability 1/2 rather than 1/3.
    let player_queue = priority_buckets.into_iter().flat_map(|(_, bucket)| {
        let mut bucket = bucket.map(|(p, _)| p).collect_vec();
        bucket.shuffle(rng);
        bucket
    });

    let mut players_per_team = enum_map! { _ => vec![] };
    let mut random_players = vec![];
    for p in player_queue {
        match p.active_faction {
            Faction::Fixed(team) => {
                if players_per_team[team].len() < TOTAL_ENVOYS_PER_TEAM {
                    players_per_team[team].push(p);
                }
            }
            Faction::Random => {
                random_players.push(p);
            }
            Faction::Observer => unreachable!(),
        }
        let total_players =
            players_per_team.values().map(|v| v.len()).sum::<usize>() + random_players.len();
        if total_players == TOTAL_ENVOYS {
            break;
        }
    }

    if let Some(current) = current_assignment {
        let mut i = 0;
        while i < random_players.len() {
            let p = random_players[i];
            if let Some(id) = current.get(&p.name)
                && players_per_team[id.team()].len() < TOTAL_ENVOYS_PER_TEAM
                && (!players_per_team[id.team().opponent()].is_empty() || random_players.len() > 1)
            {
                players_per_team[id.team()].push(p);
                random_players.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    if !random_players.iter().map(|p| p.double_games_played).all_equal() {
        // Try to balance the number of times each person double plays. This usually happens
        // when we have three players (with four+ players people typically don't double play and
        // with two people both players always double play).
        random_players.sort_by_key(|p| cmp::Reverse(p.double_games_played));
        let smaller_team = Team::iter().min_by_key(|&team| players_per_team[team].len()).unwrap();
        let larger_team = smaller_team.opponent();
        players_per_team[smaller_team].push(random_players.pop().unwrap());
        for p in random_players {
            let team = if players_per_team[larger_team].len() < TOTAL_ENVOYS_PER_TEAM {
                larger_team
            } else {
                smaller_team
            };
            players_per_team[team].push(p);
        }
    } else {
        for p in random_players {
            // Note. Although the players are already shuffled, we still need to randomize the team.
            // If we always started with, say, Red team, then in case of (Blue, Random, Random) the
            // first player would always play on two boards.
            let mut team = if rng.gen() { Team::Red } else { Team::Blue };
            if players_per_team[team].len() >= TOTAL_ENVOYS_PER_TEAM
                || players_per_team[team.opponent()].is_empty()
            {
                team = team.opponent();
            }
            players_per_team[team].push(p);
        }
    }

    players_per_team
        .into_iter()
        .flat_map(|(team, mut team_players)| {
            // Another shuffle. Since players with fixed teams are added first, we need it to make
            // sure forces are distributed evenly between players with fixed and dynamic teams.
            team_players.shuffle(rng);

            match team_players.len() {
                0 => panic!("Empty team: {:?}", team),
                1 => {
                    // TODO: `assert!(!need_to_double_play);`
                    vec![PlayerInGame {
                        name: team_players.into_iter().exactly_one().unwrap().name.clone(),
                        id: BughousePlayer::DoublePlayer(team),
                    }]
                }
                2 => {
                    let mut players = BughouseBoard::iter()
                        .zip_eq(team_players)
                        .map(move |(board_idx, participant)| PlayerInGame {
                            name: participant.name.clone(),
                            id: BughousePlayer::SinglePlayer(BughouseEnvoy {
                                board_idx,
                                force: get_bughouse_force(team, board_idx),
                            }),
                        })
                        .collect_vec();
                    let need_swap = players.iter().any(|p| {
                        current_assignment
                            .and_then(|current| current.get(&p.name))
                            .map_or(false, |&id| id.is_single_player() && id != p.id)
                    });
                    if need_swap {
                        let name_a = mem::take(&mut players[0].name);
                        let name_b = mem::take(&mut players[1].name);
                        players[0].name = name_b;
                        players[1].name = name_a;
                    }
                    players
                }
                _ => panic!("Too many players: {:?}", team_players),
            }
        })
        .collect_vec()
}


#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::envoy;
    use crate::force::Force;
    use crate::game::{double_player, single_player};
    use crate::half_integer::HalfU32;
    use crate::rules::{ChessRules, MatchRules};
    use crate::test_util::deterministic_rng;

    // Number of times to run a calculation in order to make sure an invariant holds for any random
    // input.
    const SINGLE_TEST_ITERATIONS: usize = 10;

    type Participants = HashMap<String, Participant>;

    impl IterableMut<Participant> for Participants {
        fn get_iter(&self) -> impl Iterator<Item = &Participant> { self.values() }
        fn get_iter_mut(&mut self) -> impl Iterator<Item = &mut Participant> { self.values_mut() }
    }

    trait ParticipantsExt {
        fn add(&mut self, name: &str, faction: Faction);
        fn add_with_readiness(&mut self, name: &str, faction: Faction, is_ready: bool);
    }

    impl ParticipantsExt for Participants {
        fn add(&mut self, name: &str, faction: Faction) {
            self.insert(name.to_owned(), Participant {
                name: name.to_owned(),
                is_registered_user: false,
                active_faction: faction,
                desired_faction: faction,
                games_played: 0,
                games_missed: 0,
                double_games_played: 0,
                individual_score: HalfU32::ZERO,
                is_online: true,
                is_ready: true,
            });
        }

        fn add_with_readiness(&mut self, name: &str, faction: Faction, is_ready: bool) {
            self.insert(name.to_owned(), Participant {
                name: name.to_owned(),
                is_registered_user: false,
                active_faction: faction,
                desired_faction: faction,
                games_played: 0,
                games_missed: 0,
                double_games_played: 0,
                individual_score: HalfU32::ZERO,
                is_online: true,
                is_ready,
            });
        }
    }

    #[derive(Debug, Default)]
    struct ParticipantStats {
        played_for_single_force: EnumMap<Force, usize>,
        played_for_team: EnumMap<Team, usize>,
    }

    type ParticipantStatsMap = HashMap<String, ParticipantStats>;

    fn players_to_map(players: Vec<PlayerInGame>) -> HashMap<String, BughousePlayer> {
        players.into_iter().map(|p| (p.name, p.id)).collect()
    }

    fn make_rules(rated: bool) -> Rules {
        Rules {
            chess_rules: ChessRules::bughouse_international5(),
            match_rules: MatchRules { rated },
        }
    }

    fn simulate_play(players: &[PlayerInGame], participants: &mut Participants) {
        let players: HashMap<_, _> = players.iter().map(|p| (p.name.clone(), p)).collect();
        for (_, p) in participants.iter_mut() {
            if let Some(pl) = players.get(&p.name) {
                p.games_played += 1;
                if matches!(pl.id, BughousePlayer::DoublePlayer(_)) {
                    p.double_games_played += 1;
                }
            } else if p.active_faction.is_player() {
                p.games_missed += 1;
            }
        }
    }

    fn collect_stats(players: &[PlayerInGame], stats: &mut ParticipantStatsMap) {
        for player in players {
            let st = stats.entry(player.name.clone()).or_default();
            match player.id {
                BughousePlayer::DoublePlayer(team) => {
                    st.played_for_team[team] += 1;
                }
                BughousePlayer::SinglePlayer(envoy) => {
                    st.played_for_team[envoy.team()] += 1;
                    st.played_for_single_force[envoy.force] += 1;
                }
            }
        }
    }

    macro_rules! assert_close {
        ($lhs:expr, $rhs:literal, $($arg:tt)+) => {{
            let lhs = $lhs;
            let rhs = $rhs;
            // This is for random tests, no floating point errors, so the marging should be big.
            let margin = rhs / 10;
            assert!(lhs.abs_diff(rhs) < margin, $($arg)+);
        }};
    }

    #[test]
    fn three_random_players() {
        let mut participants = Participants::new();
        participants.add_with_readiness("p1", Faction::Random, false);
        participants.add_with_readiness("p2", Faction::Random, false);
        participants.add_with_readiness("p3", Faction::Random, false);
        assert_eq!(
            verify_participants(&make_rules(true), participants.values()),
            ParticipantsStatus::CannotStart(ParticipantsError::RatedDoublePlay)
        );
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus::CanStart {
                players_ready: false,
                warning: Some(ParticipantsWarning::NeedToDoublePlay),
            }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::DynamicTeams);
        assert!(participants.values().all(|p| p.active_faction == Faction::Random));
    }

    #[test]
    fn three_vs_one() {
        let mut participants = Participants::new();
        participants.add_with_readiness("p1", Faction::Fixed(Team::Red), true);
        participants.add_with_readiness("p2", Faction::Fixed(Team::Blue), true);
        participants.add_with_readiness("p3", Faction::Fixed(Team::Blue), true);
        participants.add_with_readiness("p4", Faction::Fixed(Team::Blue), true);
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus::CanStart {
                players_ready: true,
                warning: Some(ParticipantsWarning::NeedToDoublePlayAndSeatOut),
            }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
    }

    #[test]
    fn two_players_fixable() {
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Random);
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
        assert_eq!(participants["p1"].active_faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p2"].active_faction, Faction::Fixed(Team::Blue));
    }

    #[test]
    fn three_players_fixable() {
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Red));
        participants.add("p3", Faction::Random);
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
        assert_eq!(participants["p1"].active_faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p2"].active_faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p3"].active_faction, Faction::Fixed(Team::Blue));
    }

    #[test]
    fn three_players_unfixable() {
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Random);
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::DynamicTeams);
    }

    #[test]
    fn four_players_unfixable() {
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Blue));
        participants.add("p3", Faction::Random);
        participants.add("p4", Faction::Random);
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus::CanStart { players_ready: true, warning: None }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::DynamicTeams);
    }

    #[test]
    fn four_players_fixable() {
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Blue));
        participants.add("p3", Faction::Fixed(Team::Blue));
        participants.add("p4", Faction::Random);
        assert_eq!(
            verify_participants(&make_rules(false), participants.values()),
            ParticipantsStatus::CanStart { players_ready: true, warning: None }
        );
        assert_eq!(fix_teams_if_needed(&mut participants), Teaming::FixedTeams);
        assert_eq!(participants["p1"].active_faction, Faction::Fixed(Team::Red));
        assert_eq!(participants["p2"].active_faction, Faction::Fixed(Team::Blue));
        assert_eq!(participants["p3"].active_faction, Faction::Fixed(Team::Blue));
        assert_eq!(participants["p4"].active_faction, Faction::Fixed(Team::Red));
    }

    #[test]
    fn assign_board_respects_fixed_teams() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Blue));
        participants.add("p3", Faction::Fixed(Team::Blue));
        for _ in 0..SINGLE_TEST_ITERATIONS {
            let players = players_to_map(assign_boards(participants.values(), None, rng));
            assert!(players["p1"] == BughousePlayer::DoublePlayer(Team::Red));
            let p2 = players["p2"].as_single_player().unwrap();
            let p3 = players["p3"].as_single_player().unwrap();
            assert_eq!(p2.team(), Team::Blue);
            assert_eq!(p3.team(), Team::Blue);
            assert_ne!(p2.board_idx, p3.board_idx);
        }
    }

    // Not making people double play if they don't have to is the first priority. Even above
    // balancing the number of games played.
    #[test]
    fn assign_board_no_unnecessary_double_play() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Red));
        participants.add("p3", Faction::Fixed(Team::Blue));
        participants.add("p4", Faction::Fixed(Team::Blue));
        participants.add("p5", Faction::Fixed(Team::Blue));
        participants.add("p6", Faction::Fixed(Team::Blue));
        for _ in 0..120 {
            let players = assign_boards(participants.values(), None, rng);
            simulate_play(&players, &mut participants);
        }
        for name in ["p1", "p2"] {
            let p = &participants[name];
            assert_eq!(p.games_played, 120);
            assert_eq!(p.double_games_played, 0);
        }
        for name in ["p3", "p4", "p5", "p6"] {
            let p = &participants[name];
            assert_eq!(p.games_played, 60);
            assert_eq!(p.double_games_played, 0);
        }
    }

    #[test]
    fn assign_board_balances_seating_out() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Random);
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Random);
        participants.add("p4", Faction::Random);
        participants.add("p5", Faction::Random);
        for _ in 0..100 {
            let players = assign_boards(participants.values(), None, rng);
            simulate_play(&players, &mut participants);
        }
        for p in participants.values() {
            assert_eq!(p.games_played, 80);
            assert_eq!(p.double_games_played, 0);
        }
    }

    #[test]
    fn assign_board_balances_double_play() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Random);
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Random);
        for _ in 0..120 {
            let players = assign_boards(participants.values(), None, rng);
            simulate_play(&players, &mut participants);
        }
        for p in participants.values() {
            assert_eq!(p.games_played, 120);
            assert_eq!(p.double_games_played, 40);
        }
    }

    #[test]
    fn assign_board_randomizes_evenly_with_all_random() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Random);
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Random);
        participants.add("p4", Faction::Random);
        let mut stats = ParticipantStatsMap::new();
        for _ in 0..1000 {
            let players = assign_boards(participants.values(), None, rng);
            collect_stats(&players, &mut stats);
            simulate_play(&players, &mut participants);
        }
        for p in participants.values() {
            let st = &stats[&p.name];
            assert_close!(st.played_for_single_force[Force::White], 500, "{stats:?}");
            assert_close!(st.played_for_single_force[Force::Black], 500, "{stats:?}");
            assert_close!(st.played_for_team[Team::Red], 500, "{stats:?}");
            assert_close!(st.played_for_team[Team::Blue], 500, "{stats:?}");
        }
    }

    #[test]
    fn assign_board_randomizes_evenly_with_all_fixed() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Red));
        participants.add("p3", Faction::Fixed(Team::Blue));
        participants.add("p4", Faction::Fixed(Team::Blue));
        let mut stats = ParticipantStatsMap::new();
        for _ in 0..1000 {
            let players = assign_boards(participants.values(), None, rng);
            collect_stats(&players, &mut stats);
            simulate_play(&players, &mut participants);
        }
        for p in participants.values() {
            let st = &stats[&p.name];
            assert_close!(st.played_for_single_force[Force::White], 500, "{stats:?}");
            assert_close!(st.played_for_single_force[Force::Black], 500, "{stats:?}");
        }
    }

    #[test]
    fn assign_board_randomizes_evenly_with_partially_fixed_three_players() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Blue));
        participants.add("p3", Faction::Random);
        let mut stats = ParticipantStatsMap::new();
        for _ in 0..2000 {
            let players = assign_boards(participants.values(), None, rng);
            collect_stats(&players, &mut stats);
            simulate_play(&players, &mut participants);
        }
        for name in ["p1", "p2"] {
            let p = &participants[name];
            let st = &stats[name];
            assert_close!(p.double_games_played, 1000, "{participants:?}");
            assert_close!(st.played_for_single_force[Force::White], 500, "{stats:?}");
            assert_close!(st.played_for_single_force[Force::Black], 500, "{stats:?}");
        }
        {
            let name = "p3";
            let p = &participants[name];
            let st = &stats[name];
            assert_eq!(p.double_games_played, 0, "{participants:?}");
            assert_close!(st.played_for_single_force[Force::White], 1000, "{stats:?}");
            assert_close!(st.played_for_single_force[Force::Black], 1000, "{stats:?}");
            assert_close!(st.played_for_team[Team::Red], 1000, "{stats:?}");
            assert_close!(st.played_for_team[Team::Blue], 1000, "{stats:?}");
        }
    }

    #[test]
    fn assign_board_randomizes_evenly_with_partially_fixed_five_players() {
        let rng = &mut deterministic_rng();
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Blue));
        participants.add("p3", Faction::Random);
        participants.add("p4", Faction::Random);
        participants.add("p5", Faction::Random);
        let mut stats = ParticipantStatsMap::new();
        for _ in 0..1000 {
            let players = assign_boards(participants.values(), None, rng);
            collect_stats(&players, &mut stats);
            simulate_play(&players, &mut participants);
        }
        for p in participants.values() {
            let st = &stats[&p.name];
            assert_close!(st.played_for_single_force[Force::White], 400, "{stats:?}");
            assert_close!(st.played_for_single_force[Force::Black], 400, "{stats:?}");
        }
        for name in ["p3", "p4", "p5"] {
            let st = &stats[name];
            assert_close!(st.played_for_team[Team::Red], 400, "{stats:?}");
            assert_close!(st.played_for_team[Team::Blue], 400, "{stats:?}");
        }
    }

    #[test]
    fn reassignment_is_idempotent_when_possible() {
        let rng = &mut deterministic_rng();
        let current_assignment = [
            single_player("p1", envoy!(White A)),
            single_player("p5", envoy!(Black B)),
            single_player("p7", envoy!(Black A)),
            single_player("p8", envoy!(White B)),
        ];
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Fixed(Team::Red));
        participants.add("p3", Faction::Fixed(Team::Blue));
        participants.add("p4", Faction::Random);
        participants.add("p5", Faction::Random);
        participants.add("p6", Faction::Random);
        participants.add("p7", Faction::Random);
        participants.add("p8", Faction::Random);
        for _ in 0..SINGLE_TEST_ITERATIONS {
            let mut players = assign_boards(participants.values(), Some(&current_assignment), rng);
            players.sort_by_key(|p| p.name.clone());
            assert_eq!(players, current_assignment);
        }
    }

    #[test]
    fn reassignment_keeps_existing_players() {
        let rng = &mut deterministic_rng();
        let current_assignment = [
            single_player("p1", envoy!(White A)),
            single_player("p2", envoy!(Black B)),
            single_player("p3", envoy!(Black A)),
            single_player("p4", envoy!(White B)),
        ];
        let mut participants = Participants::new();
        participants.add("p1", Faction::Random);
        participants.add("p3", Faction::Random);
        participants.add("p4", Faction::Random);
        participants.add("p5", Faction::Random);
        for _ in 0..SINGLE_TEST_ITERATIONS {
            let mut players = assign_boards(participants.values(), Some(&current_assignment), rng);
            players.sort_by_key(|p| p.name.clone());
            assert_eq!(players, [
                single_player("p1", envoy!(White A)),
                single_player("p3", envoy!(Black A)),
                single_player("p4", envoy!(White B)),
                single_player("p5", envoy!(Black B)),
            ]);
        }
    }

    #[test]
    fn reassignment_keeps_team_when_possible() {
        let rng = &mut deterministic_rng();
        let current_assignment = [single_player("p2", envoy!(White A))];
        let mut participants = Participants::new();
        participants.add("p1", Faction::Fixed(Team::Red));
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Random);
        for _ in 0..SINGLE_TEST_ITERATIONS {
            let mut players = assign_boards(participants.values(), Some(&current_assignment), rng);
            players.sort_by_key(|p| p.name.clone());
            assert_eq!(players, [
                single_player("p1", envoy!(Black B)),
                single_player("p2", envoy!(White A)),
                double_player("p3", Team::Blue),
            ]);
        }
    }

    #[test]
    fn reassignment_breaks_team_if_needed() {
        let rng = &mut deterministic_rng();
        let current_assignment = [
            single_player("p1", envoy!(White A)),
            single_player("p2", envoy!(Black B)),
            double_player("p3", Team::Blue),
        ];
        let mut participants = Participants::new();
        participants.add("p1", Faction::Random);
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Observer);
        for _ in 0..SINGLE_TEST_ITERATIONS {
            let mut players = assign_boards(participants.values(), Some(&current_assignment), rng);
            players.sort_by_key(|p| p.name.clone());
            for p in &players {
                assert!(p.id.is_double_player());
            }
            let (p1, p2) = players.into_iter().collect_tuple().unwrap();
            assert_ne!(p1.id.team(), p2.id.team());
        }
    }


    #[test]
    fn reassignment_changes_team_if_needed() {
        let rng = &mut deterministic_rng();
        let current_assignment = [
            single_player("p1", envoy!(White A)),
            single_player("p2", envoy!(Black B)),
            single_player("p3", envoy!(White B)),
            single_player("p4", envoy!(Black A)),
        ];
        let mut participants = Participants::new();
        participants.add("p1", Faction::Random);
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Random);
        participants.add("p5", Faction::Fixed(Team::Red));
        let mut stats = ParticipantStatsMap::new();
        for _ in 0..1000 {
            let players = assign_boards(participants.values(), Some(&current_assignment), rng);
            collect_stats(&players, &mut stats);
            simulate_play(&players, &mut participants);
        }
        assert_close!(stats["p3"].played_for_team[Team::Blue], 1000, "{stats:?}");
        assert_close!(stats["p5"].played_for_team[Team::Red], 1000, "{stats:?}");
        // Fixed team condition for p5 must be satisfied, so either p1 or p2 is pushed to another
        // team.
        for name in ["p1", "p2"] {
            assert_close!(stats[name].played_for_team[Team::Red], 500, "{stats:?}");
            assert_close!(stats[name].played_for_team[Team::Blue], 500, "{stats:?}");
        }
    }

    #[test]
    fn reassignment_adds_players_if_double_play() {
        let rng = &mut deterministic_rng();
        let current_assignment = [
            single_player("p1", envoy!(White A)),
            single_player("p2", envoy!(Black B)),
            double_player("p3", Team::Blue),
        ];
        let mut participants = Participants::new();
        participants.add("p1", Faction::Random);
        participants.add("p2", Faction::Random);
        participants.add("p3", Faction::Random);
        participants.add("p4", Faction::Random);
        participants.add("p5", Faction::Random);
        for _ in 0..1000 {
            let players = assign_boards(participants.values(), Some(&current_assignment), rng);
            assert!(players.iter().all(|p| p.id.is_single_player()));
            simulate_play(&players, &mut participants);
        }
        // One of the new players joins to avoid double-play when not needed.
        for name in ["p4", "p5"] {
            assert_close!(participants[name].games_played, 500, "{participants:?}");
        }
    }
}
