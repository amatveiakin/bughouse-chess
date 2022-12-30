// TODO: process stats incrementally and persist in the DB.

use std::collections::HashMap;

use log::error;
use skillratings::{
    elo::{elo, EloConfig, EloRating},
    weng_lin::{weng_lin, weng_lin_two_teams, WengLinConfig, WengLinRating},
    Outcomes,
};

use bughouse_chess::persistence::GameResultRow;

type Rating = WengLinRating;

// TODO: persist the history of these stats.
#[derive(Clone, Copy, Debug)]
pub struct RawStats {
    pub wins: usize,
    pub losses: usize,
    pub draws: usize,
    pub elo: Option<EloRating>,
    pub rating: Option<Rating>,
}

impl RawStats {
    pub fn update(
        &self,
        outcome: Outcomes,
        new_elo: Option<EloRating>,
        new_rating: Option<Rating>,
    ) -> Self {
        Self {
            wins: self.wins + (outcome == Outcomes::WIN) as usize,
            losses: self.losses + (outcome == Outcomes::LOSS) as usize,
            draws: self.draws + (outcome == Outcomes::DRAW) as usize,
            elo: new_elo,
            rating: new_rating,
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
        }
    }
}

#[derive(Default)]
pub struct GroupStats {
    pub per_player: HashMap<String, RawStats>,
    pub per_team: HashMap<[String; 2], RawStats>,
}

struct GameStats {
    red_team: RawStats,
    blue_team: RawStats,
    red_players: [RawStats; 2],
    blue_players: [RawStats; 2],
}

fn default_elo() -> EloRating {
    EloRating { rating: 1600.0 }
}

fn default_weng_lin() -> WengLinRating {
    WengLinRating::new()
}

fn process_game(result: &str, prior_stats: GameStats) -> GameStats {
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
        &prior_stats
            .blue_team
            .rating
            .unwrap_or_else(default_weng_lin),
        &red_outcome,
        &WengLinConfig {
            //beta: 240.0, // To mimic general ELO rating values, 80% win rate ~ +240 ELO
            ..Default::default()
        },
    );
    let (red_players_rating, blue_players_rating) = weng_lin_two_teams(
        &prior_stats
            .red_players
            .map(|p| p.rating.unwrap_or_else(default_weng_lin)),
        &prior_stats
            .blue_players
            .map(|p| p.rating.unwrap_or_else(default_weng_lin)),
        &red_outcome,
        &WengLinConfig {
            beta: 240.0, // To mimic general ELO rating values, 80% win rate ~ +240 ELO
            ..Default::default()
        },
    );
    GameStats {
        red_team: prior_stats.red_team.update(
            red_outcome,
            Some(red_team_elo),
            Some(red_team_rating),
        ),
        blue_team: prior_stats.blue_team.update(
            blue_outcome,
            Some(blue_team_elo),
            Some(blue_team_rating),
        ),
        red_players: [
            prior_stats.red_players[0].update(red_outcome, None, Some(red_players_rating[0])),
            prior_stats.red_players[1].update(red_outcome, None, Some(red_players_rating[1])),
        ],
        blue_players: [
            prior_stats.blue_players[0].update(blue_outcome, None, Some(blue_players_rating[0])),
            prior_stats.blue_players[1].update(blue_outcome, None, Some(blue_players_rating[1])),
        ],
    }
}

impl GroupStats {
    pub fn update(&mut self, game: &GameResultRow) -> anyhow::Result<()> {
        let red_team = sort([game.player_red_a.clone(), game.player_red_b.clone()]);
        let blue_team = sort([game.player_blue_a.clone(), game.player_blue_b.clone()]);
        let new_stats = process_game(
            game.result.as_str(),
            GameStats {
                red_team: *self.per_team.entry(red_team.clone()).or_default(),
                blue_team: *self.per_team.entry(blue_team.clone()).or_default(),
                red_players: red_team.clone().map(|p| *self.per_player.entry(p).or_default()),
                blue_players: blue_team.clone().map(|p| *self.per_player.entry(p).or_default()),
            },
        );
        *self.per_team.entry(red_team.clone()).or_default() = new_stats.red_team;
        *self.per_team.entry(blue_team.clone()).or_default() = new_stats.blue_team;
        for i in 0..red_team.len() {
            self.per_player.insert(red_team[i].clone(), new_stats.red_players[i]);
        }
        for i in 0..blue_team.len() {
            self.per_player.insert(blue_team[i].clone(), new_stats.blue_players[i]);
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
