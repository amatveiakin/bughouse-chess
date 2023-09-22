// TODO: process stats incrementally and persist in the DB.

use std::collections::HashMap;

use log::error;
use skillratings::elo::{self, elo, EloConfig, EloRating};
use skillratings::weng_lin::{self, weng_lin, weng_lin_two_teams, WengLinConfig, WengLinRating};
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
    // Returns average points per game or 0.5 if no games were played.
    pub fn pointrate(&self) -> f64 {
        let count = self.wins + self.losses + self.draws;
        if count == 0 {
            0.5
        } else {
            (self.wins as f64 + 0.5 * self.draws as f64) / (count as f64)
        }
    }
}

#[allow(clippy::derivable_impls)]
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
    pub meta_stats: Vec<MetaStats>,
}

#[derive(Default, Clone, Copy)]
pub struct MetaStats {
    pub player_rating_predictor_loss_sum: f64,
    pub team_rating_predictor_loss_sum: f64,
    pub team_elo_predictor_loss_sum: f64,
    pub team_pointrate_predictor_loss_sum: f64,
    pub game_count: usize,
}

struct GameStats {
    red_team: RawStats,
    blue_team: RawStats,
    red_players: [RawStats; 2],
    blue_players: [RawStats; 2],
    meta_stats: Option<MetaStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeMetaStats {
    No,
    Yes,
}

fn default_elo() -> EloRating { EloRating { rating: 1600.0 } }

fn default_weng_lin() -> WengLinRating { WengLinRating::new() }

fn process_game(
    result: &str, prior_stats: GameStats, game_end_time: Option<OffsetDateTime>,
    update_index: usize, compute_meta_stats: ComputeMetaStats,
) -> GameStats {
    let (red_outcome, blue_outcome, actual_red_score, actual_blue_score) = match result {
        "DRAW" => (Outcomes::DRAW, Outcomes::DRAW, 0.5, 0.5),
        "VICTORY_RED" => (Outcomes::WIN, Outcomes::LOSS, 1.0, 0.0),
        "VICTORY_BLUE" => (Outcomes::LOSS, Outcomes::WIN, 0.0, 1.0),
        _ => {
            error!("Ignoring unrecognized game result '{}'", result);
            return prior_stats;
        }
    };
    let prior_red_team_elo = prior_stats.red_team.elo.unwrap_or_else(default_elo);
    let prior_blue_team_elo = prior_stats.blue_team.elo.unwrap_or_else(default_elo);
    let (red_team_elo, blue_team_elo) =
        elo(&prior_red_team_elo, &prior_blue_team_elo, &red_outcome, &EloConfig { k: 20.0 });

    let prior_red_team_rating = prior_stats.red_team.rating.unwrap_or_else(default_weng_lin);
    let prior_blue_team_rating = prior_stats.blue_team.rating.unwrap_or_else(default_weng_lin);
    let (red_team_rating, blue_team_rating) = weng_lin(
        &prior_red_team_rating,
        &prior_blue_team_rating,
        &red_outcome,
        &WengLinConfig::default(),
    );

    let prior_red_players_ratings =
        prior_stats.red_players.map(|p| p.rating.unwrap_or_else(default_weng_lin));
    let prior_blue_players_ratings =
        prior_stats.blue_players.map(|p| p.rating.unwrap_or_else(default_weng_lin));
    let (red_players_rating, blue_players_rating) = weng_lin_two_teams(
        &prior_red_players_ratings,
        &prior_blue_players_ratings,
        &red_outcome,
        &WengLinConfig::default(),
    );

    let meta_stats = match compute_meta_stats {
        ComputeMetaStats::No => None,
        ComputeMetaStats::Yes => {
            let prior_red_team_pointrate = prior_stats.red_team.pointrate();
            let prior_blue_team_pointrate = prior_stats.blue_team.pointrate();
            let sum_pointrate = prior_red_team_pointrate + prior_blue_team_pointrate;

            let (red_team_pointrate_expected_score, blue_team_pointrate_expected_score) =
                if sum_pointrate == 0.0 {
                    (0.5, 0.5)
                } else {
                    (
                        prior_red_team_pointrate / sum_pointrate,
                        prior_blue_team_pointrate / sum_pointrate,
                    )
                };
            let (elo_expected_red_score, elo_expected_blue_score) =
                elo::expected_score(&prior_red_team_elo, &prior_blue_team_elo);
            let (expected_red_team_score, expected_blue_team_score) = weng_lin::expected_score(
                &prior_red_team_rating,
                &prior_blue_team_rating,
                &WengLinConfig::default(),
            );
            let (expected_red_players_score, expected_blue_players_score) =
                weng_lin::expected_score_teams(
                    &prior_red_players_ratings,
                    &prior_blue_players_ratings,
                    &WengLinConfig::default(),
                );
            let mut ms = prior_stats.meta_stats.unwrap_or_default();
            ms.game_count += 1;
            ms.player_rating_predictor_loss_sum += predictor_loss_function(
                expected_red_players_score,
                expected_blue_players_score,
                actual_red_score,
                actual_blue_score,
            );
            ms.team_rating_predictor_loss_sum += predictor_loss_function(
                expected_red_team_score,
                expected_blue_team_score,
                actual_red_score,
                actual_blue_score,
            );
            ms.team_elo_predictor_loss_sum += predictor_loss_function(
                elo_expected_red_score,
                elo_expected_blue_score,
                actual_red_score,
                actual_blue_score,
            );
            ms.team_pointrate_predictor_loss_sum += predictor_loss_function(
                red_team_pointrate_expected_score,
                blue_team_pointrate_expected_score,
                actual_red_score,
                actual_blue_score,
            );
            Some(ms)
        }
    };

    GameStats {
        meta_stats,
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

#[allow(clippy::ptr_arg)]
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
    #[allow(clippy::needless_range_loop)]
    pub fn update(
        &mut self, game: &GameResultRow, compute_meta_stats: ComputeMetaStats,
    ) -> anyhow::Result<()> {
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
                meta_stats: self.meta_stats.last().cloned(),
            },
            game.game_end_time,
            self.update_index,
            compute_meta_stats,
        );
        self.update_team(&red_team, new_stats.red_team);
        self.update_team(&blue_team, new_stats.blue_team);
        for i in 0..red_team.len() {
            self.update_player(&red_team[i], new_stats.red_players[i]);
        }
        for i in 0..blue_team.len() {
            self.update_player(&blue_team[i], new_stats.blue_players[i]);
        }
        if let Some(new_meta_stats) = new_stats.meta_stats {
            self.meta_stats.push(new_meta_stats);
        }
        Ok(())
    }
}

fn predictor_loss_function(expected1: f64, expected2: f64, actual1: f64, actual2: f64) -> f64 {
    // In reality expected1 + expected2 = actual1 + actual2 = 1.
    // But we keep it this way for symmetry & in case the game becomes
    // not 100% antagonistic.
    0.5 * ((expected1 - actual1).powi(2) + (expected2 - actual2).powi(2))
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
