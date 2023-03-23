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

mod algebraic;
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
pub mod janitor;
pub mod lobby;
pub mod meter;
pub mod ping_pong;
pub mod pgn;
pub mod server;
pub mod server_helpers;
pub mod server_hooks;
pub mod session;
pub mod session_store;
pub mod test_util;
pub mod util;

pub use algebraic::*;
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
