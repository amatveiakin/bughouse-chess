// TODO: process stats incrementally and persist in the DB.

use std::collections::HashMap;

use log::error;
use skillratings::elo::{elo, EloConfig, EloRating};
use skillratings::weng_lin::{weng_lin, weng_lin_two_teams, WengLinConfig, WengLinRating};
use skillratings::Outcomes;
use time::OffsetDateTime;

use crate::persistence::GameResultRow;

type Rating = WengLinRating;

// TODO: persist the history of these stats.
#[derive(Clone, Copy, Debug)]
pub struct RawStats {
    pub wins: usize,
    pub losses: usize,
    pub draws: usize,
    pub elo: Option<EloRating>,
    pub rating: Option<Rating>,
    pub last_update: Option<OffsetDateTime>,
    // The index of the event that resulted in this update.
    // This does not neccessarily coincide with row id of
    // the underlying tables.
    pub update_index: usize,
}

impl RawStats {
    pub fn update(
        &self, outcome: Outcomes, new_elo: Option<EloRating>, new_rating: Option<Rating>,
        new_last_update: Option<OffsetDateTime>, new_update_index: usize,
    ) -> Self {
        Self {
            wins: self.wins + (outcome == Outcomes::WIN) as usize,
            losses: self.losses + (outcome == Outcomes::LOSS) as usize,
            draws: self.draws + (outcome == Outcomes::DRAW) as usize,
            elo: new_elo,
            rating: new_rating,
            last_update: new_last_update,
            update_index: new_update_index,
        }
    }
}

impl Default for RawStats {
    fn default() -> Self {
        RawStats {
            wins: 0,
            losses: 0,
            draws: 0,
            elo: None,
            rating: None,
            last_update: None,
            update_index: 0,
        }
    }
}

#[derive(Default)]
pub struct GroupStats<Stats> {
    pub per_player: HashMap<String, Stats>,
    pub per_team: HashMap<[String; 2], Stats>,
    pub update_index: usize,
}

struct GameStats {
    red_team: RawStats,
    blue_team: RawStats,
    red_players: [RawStats; 2],
    blue_players: [RawStats; 2],
}

fn default_elo() -> EloRating { EloRating { rating: 1600.0 } }

fn default_weng_lin() -> WengLinRating { WengLinRating::new() }

fn process_game(
    result: &str, prior_stats: GameStats, game_end_time: Option<OffsetDateTime>,
    update_index: usize,
) -> GameStats {
    let (red_outcome, blue_outcome) = match result {
        "DRAW" => (Outcomes::DRAW, Outcomes::DRAW),
        "VICTORY_RED" => (Outcomes::WIN, Outcomes::LOSS),
        "VICTORY_BLUE" => (Outcomes::LOSS, Outcomes::WIN),
        _ => {
            error!("Ignoring unrecognized game result '{}'", result);
            return prior_stats;
        }
    };
    let (red_team_elo, blue_team_elo) = elo(
        &prior_stats.red_team.elo.unwrap_or_else(default_elo),
        &prior_stats.blue_team.elo.unwrap_or_else(default_elo),
        &red_outcome,
        &EloConfig { k: 20.0 },
    );
    let (red_team_rating, blue_team_rating) = weng_lin(
        &prior_stats.red_team.rating.unwrap_or_else(default_weng_lin),
        &prior_stats.blue_team.rating.unwrap_or_else(default_weng_lin),
        &red_outcome,
        &WengLinConfig::default(),
    );
    let (red_players_rating, blue_players_rating) = weng_lin_two_teams(
        &prior_stats.red_players.map(|p| p.rating.unwrap_or_else(default_weng_lin)),
        &prior_stats.blue_players.map(|p| p.rating.unwrap_or_else(default_weng_lin)),
        &red_outcome,
        &WengLinConfig::default(),
    );
    GameStats {
        red_team: prior_stats.red_team.update(
            red_outcome,
            Some(red_team_elo),
            Some(red_team_rating),
            game_end_time,
            update_index,
        ),
        blue_team: prior_stats.blue_team.update(
            blue_outcome,
            Some(blue_team_elo),
            Some(blue_team_rating),
            game_end_time,
            update_index,
        ),
        red_players: [
            prior_stats.red_players[0].update(
                red_outcome,
                None,
                Some(red_players_rating[0]),
                game_end_time,
                update_index,
            ),
            prior_stats.red_players[1].update(
                red_outcome,
                None,
                Some(red_players_rating[1]),
                game_end_time,
                update_index,
            ),
        ],
        blue_players: [
            prior_stats.blue_players[0].update(
                blue_outcome,
                None,
                Some(blue_players_rating[0]),
                game_end_time,
                update_index,
            ),
            prior_stats.blue_players[1].update(
                blue_outcome,
                None,
                Some(blue_players_rating[1]),
                game_end_time,
                update_index,
            ),
        ],
    }
}

pub trait StatStore {
    fn get_team(&self, team: &[String; 2]) -> RawStats;
    fn get_player(&self, player: &String) -> RawStats;
    fn update_team(&mut self, team: &[String; 2], stats: RawStats);
    fn update_player(&mut self, player: &String, stats: RawStats);
}

impl StatStore for GroupStats<RawStats> {
    fn get_team(&self, team: &[String; 2]) -> RawStats {
        self.per_team.get(team).cloned().unwrap_or_default()
    }
    fn get_player(&self, player: &String) -> RawStats {
        self.per_player.get(player).cloned().unwrap_or_default()
    }
    fn update_team(&mut self, team: &[String; 2], stats: RawStats) {
        self.per_team.insert(team.clone(), stats);
    }
    fn update_player(&mut self, player: &String, stats: RawStats) {
        self.per_player.insert(player.clone(), stats);
    }
}

impl StatStore for GroupStats<Vec<RawStats>> {
    fn get_team(&self, team: &[String; 2]) -> RawStats {
        self.per_team.get(team).and_then(|v| v.last()).cloned().unwrap_or_default()
    }

    fn get_player(&self, player: &String) -> RawStats {
        self.per_player.get(player).and_then(|v| v.last()).cloned().unwrap_or_default()
    }

    fn update_team(&mut self, team: &[String; 2], stats: RawStats) {
        self.per_team.entry(team.clone()).or_default().push(stats)
    }

    fn update_player(&mut self, player: &String, stats: RawStats) {
        self.per_player.entry(player.clone()).or_default().push(stats)
    }
}

impl<Stats> GroupStats<Stats>
where
    Self: StatStore,
{
    pub fn update(&mut self, game: &GameResultRow) -> anyhow::Result<()> {
        self.update_index += 1;
        let red_team = sort([game.player_red_a.clone(), game.player_red_b.clone()]);
        let blue_team = sort([game.player_blue_a.clone(), game.player_blue_b.clone()]);
        let new_stats = process_game(
            game.result.as_str(),
            GameStats {
                red_team: self.get_team(&red_team),
                blue_team: self.get_team(&blue_team),
                red_players: map_arr_ref(&red_team, |p| self.get_player(p)),
                blue_players: map_arr_ref(&blue_team, |p| self.get_player(p)),
            },
            game.game_end_time,
            self.update_index,
        );
        self.update_team(&red_team, new_stats.red_team);
        self.update_team(&blue_team, new_stats.blue_team);
        for i in 0..red_team.len() {
            self.update_player(&red_team[i], new_stats.red_players[i]);
        }
        for i in 0..blue_team.len() {
            self.update_player(&blue_team[i], new_stats.blue_players[i]);
        }
        Ok(())
    }
}

pub fn sort<A, T>(mut array: A) -> A
where
    A: AsMut<[T]>,
    T: Ord,
{
    array.as_mut().sort();
    array
}

// Equivalent to input.each_ref().map(f)
// each_ref is unstable
pub fn map_arr_ref<T, V, F: Fn(&T) -> V>(input: &[T; 2], f: F) -> [V; 2] {
    [f(&input[0]), f(&input[1])]
}
