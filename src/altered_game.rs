use crate::board::{Turn, TurnError};
use crate::clock::GameInstant;
use crate::game::{BughouseGameStatus, BughouseGame};


// In online multiplayer: game with local changes not (yet) confirmed by the server.
#[derive(Debug)]
pub struct AlteredGame {
    // All local turns are assumed to be made on behalf of this player.
    my_name: String,
    // State as it has been confirmed by the server.
    game_confirmed: BughouseGame,
    // Local turn, unconfirmed by the server yet, but displayed on the client.
    // This is always a valid turn for the `game_confirmed`.
    local_turn: Option<(Turn, GameInstant)>,
}

impl AlteredGame {
    pub fn new(my_name: String, game_confirmed: BughouseGame) -> Self {
        AlteredGame {
            my_name,
            game_confirmed,
            local_turn: None,
        }
    }

    // Status returned by this function may differ from `local_game()` status.
    // This function should be used as the source of truth when showing game status to the
    // user, as it's possible that the final status from the server will be different, e.g.
    // if the game ended earlier on the other board.
    pub fn status(&self) -> BughouseGameStatus {
        self.game_confirmed.status()
    }

    pub fn set_status(&mut self, status: BughouseGameStatus, time: GameInstant) {
        self.game_confirmed.set_status(status, time)
    }

    pub fn apply_remote_turn_from_algebraic(
        &mut self, player_name: &str, turn_algebraic: &str, time: GameInstant)
        -> Result<(), TurnError>
    {
        if player_name == self.my_name {
            self.local_turn = None;
        }
        self.game_confirmed.try_turn_by_player_from_algebraic(
            &player_name, &turn_algebraic, time
        )?;
        Ok(())
    }

    pub fn game_confirmed(&self) -> &BughouseGame {
        &self.game_confirmed
    }

    pub fn local_game(&self) -> BughouseGame {
        let mut game = self.game_confirmed.clone();
        if let Some((turn, turn_time)) = self.local_turn {
            // Note. Not calling `test_flag`, because only server records flag defeat.
            game.try_turn_by_player(&self.my_name, turn, turn_time).unwrap();
        }
        game
    }

    pub fn can_make_local_turn(&self) -> bool {
        self.game_confirmed.player_is_active(&self.my_name).unwrap() && self.local_turn.is_none()
    }

    pub fn try_local_turn_from_algebraic(&mut self, turn_algebraic: &str, time: GameInstant)
        -> Result<(), TurnError>
    {
        let mut game_copy = self.game_confirmed.clone();
        let turn = game_copy.try_turn_by_player_from_algebraic(
            &self.my_name, turn_algebraic, time
        )?;
        self.local_turn = Some((turn, time));
        Ok(())
    }

    pub fn reset_local_changes(&mut self) {
        self.local_turn = None;
    }
}
