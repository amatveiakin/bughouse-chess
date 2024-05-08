#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    // This is either an online game server or a standalone app for offline play. It is the source
    // of truth for the game state.
    ServerOrStandalone,

    // This is a client app that connects to a server. It tries to represent the state of the game
    // as best it could, but it is aware of its own limitations. In particular:
    //   - It never checks flag defeats and gracefully degrades when time remaining becomes
    //     negative. This is required for correctness in the face of time differences.
    //   - It doesn't store the board history for three-fold repetition draw. This is a free
    //     optimization that we can do because the server always reports game over anyway.
    Client,
}
