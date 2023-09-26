// Note. These is also `bughouse_wasm/src/bughouse_prelude.rs`.
//
// Why do we have this weird scheme where prelude is external to `bughouse_chess` crate? I like the
// idea of specifying full include paths inside `bughouse_chess` itself and I haven't found a better
// way to enforce this. If prelude is moved to `bughouse_chess`, then VSCode Rust extension
// auto-include feature prefers `crate::prelude::Foo` over `crate::foo::Foo`.
//
// What to put in prelude? The idea was to expose chess concepts directly: rules, pieces, boards,
// etc. Auxiliary concepts (networking, persistent, utils, etc.) remain behind namespaces.

pub use bughouse_chess::algebraic::*;
pub use bughouse_chess::altered_game::*;
pub use bughouse_chess::board::*;
pub use bughouse_chess::chalk::*;
pub use bughouse_chess::clock::*;
pub use bughouse_chess::coord::*;
pub use bughouse_chess::display::*;
pub use bughouse_chess::event::*;
pub use bughouse_chess::force::*;
pub use bughouse_chess::game::*;
pub use bughouse_chess::grid::*;
pub use bughouse_chess::piece::*;
pub use bughouse_chess::player::*;
pub use bughouse_chess::rules::*;
pub use bughouse_chess::scores::*;
pub use bughouse_chess::starter::*;
pub use bughouse_chess::*;
