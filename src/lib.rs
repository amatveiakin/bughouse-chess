#![forbid(unsafe_code)]

// Visibility philosophy:
//   - Chess concept (rules, pieces, boards...) are exposed directly.
//     Modules defining them should do
//       mod Foo;
//       ...
//       pub use Foo:*;
//   - Auxiliary concepts (networking, persistent, utils...) are behind namespaces.
//     Modules defining them should do
//       pub mod Foo;

mod altered_game;
mod board;
mod chalk;
mod clock;
mod coord;
mod display;
mod event;
mod force;
mod game;
mod grid;
mod piece;
mod player;
mod rules;
mod scores;
mod starter;
pub mod client;
pub mod fen;
pub mod heartbeat;
pub mod janitor;
pub mod meter;
pub mod persistence;
pub mod pgn;
pub mod server;
pub mod server_hooks;
pub mod test_util;
pub mod util;

pub use altered_game::*;
pub use board::*;
pub use chalk::*;
pub use clock::*;
pub use coord::*;
pub use display::*;
pub use event::*;
pub use force::*;
pub use game::*;
pub use grid::*;
pub use piece::*;
pub use player::*;
pub use rules::*;
pub use scores::*;
pub use starter::*;
