#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    // This is either an online game server or a standalone app for offline play. It is the source
    // of truth for the game state.
    ServerOrStandalone,

    // This is a client app that connects to a server. It tries to represent the state of the game
    // as best it could, but it is aware of its own limitations. In particular:
    //   - It never checks flag defeats and gracefully degrades when time remaining becomes
    //     negative.
    // Improvement potential: Implement other optimizations and safe-guards:
    //   - Don't store board history for three-fold repetition draw;
    //   - Don't check victory conditions at all.
    Client,
}
