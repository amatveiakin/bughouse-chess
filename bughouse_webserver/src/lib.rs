// TODO: process stats incrementally and persist in the DB.

use std::collections::HashMap;

use bughouse_chess::persistence::GameResultRow;

// TODO: persist the history of these stats.
#[derive(Clone, Copy, Debug)]
pub struct RawStats {
    pub wins: usize,
    pub losses: usize,
    pub draws: usize,
    pub elo: f64,
}

impl RawStats {
    pub fn zero() -> Self {
        Self {
            wins: 0,
            losses: 0,
            draws: 0,
            elo: 0.0,
        }
    }

    pub fn merge_from(&mut self, other: &RawStats) {
        self.wins += other.wins;
        self.losses += other.losses;
        self.draws += other.draws;
        self.elo += other.elo;
    }
}

impl Default for RawStats {
    fn default() -> Self {
        RawStats {
            wins: 0,
            losses: 0,
            draws: 0,
            elo: 1600.0,
        }
    }
}

#[derive(Default)]
pub struct GroupStats {
    pub per_player: HashMap<String, RawStats>,
    pub per_team: HashMap<[String; 2], RawStats>,
}

struct Delta {
    red_team: RawStats,
    blue_team: RawStats,
}

impl Default for Delta {
    fn default() -> Self {
        Self {
            red_team: RawStats::zero(),
            blue_team: RawStats::zero(),
        }
    }
}

fn process_game(result: &str, prior_red_team_elo: f64, prior_blue_team_elo: f64) -> Delta {
    let (red_won, blue_won, draw, red_score, blue_score) = match result {
        "DRAW" => (0, 0, 1, 0.5, 0.5),
        "VICTORY_RED" => (1, 0, 0, 1.0, 0.0),
        "VICTORY_BLUE" => (0, 1, 0, 0.0, 1.0),
        _ => return Delta::default(),
    };
    let red_elo_diff = elo_adjustment_diff(prior_red_team_elo, prior_blue_team_elo, red_score);
    let blue_elo_diff = elo_adjustment_diff(prior_red_team_elo, prior_blue_team_elo, blue_score);
    Delta {
        red_team: RawStats {
            wins: red_won,
            losses: blue_won,
            draws: draw,
            elo: red_elo_diff,
        },
        blue_team: RawStats {
            wins: blue_won,
            losses: red_won,
            draws: draw,
            elo: blue_elo_diff,
        },
    }
}

impl GroupStats {
    pub fn update(&mut self, game: &GameResultRow) -> anyhow::Result<()> {
        let red_team = sort([game.player_red_a.clone(), game.player_red_b.clone()]);
        let blue_team = sort([game.player_blue_a.clone(), game.player_blue_b.clone()]);
        let red_team_elo = self.per_team.entry(red_team.clone()).or_default().elo;
        let blue_team_elo = self.per_team.entry(blue_team.clone()).or_default().elo;
        let delta = process_game(game.result.as_str(), red_team_elo, blue_team_elo);

        self.per_team
            .entry(red_team.clone())
            .or_default()
            .merge_from(&delta.red_team);
        self.per_team
            .entry(blue_team.clone())
            .or_default()
            .merge_from(&delta.blue_team);

        // Players receive exactly the same deltas as their team.
        for player in red_team {
            self.per_player
                .entry(player)
                .or_default()
                .merge_from(&RawStats {
                    elo: delta.red_team.elo * 0.5,
                    ..delta.red_team
                });
        }
        for player in blue_team {
            self.per_player
                .entry(player)
                .or_default()
                .merge_from(&RawStats {
                    elo: delta.blue_team.elo * 0.5,
                    ..delta.blue_team
                });
        }
        Ok(())
    }
}

fn elo_score_expectation(rating: f64, opponent_rating: f64) -> f64 {
    1. / (1. + 10f64.powf((opponent_rating - rating) / 400.))
}

fn elo_adjustment_diff(rating: f64, opponent_rating: f64, score: f64) -> f64 {
    20.0 * (score - elo_score_expectation(rating, opponent_rating))
}

pub fn sort<A, T>(mut array: A) -> A
where
    A: AsMut<[T]>,
    T: Ord,
{
    array.as_mut().sort();
    array
}
