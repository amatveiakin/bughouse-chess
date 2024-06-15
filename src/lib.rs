#![forbid(unsafe_code)]
#![cfg_attr(feature = "strict", deny(warnings))]
#![feature(anonymous_lifetime_in_impl_trait)]
#![feature(fn_traits)]
#![feature(int_roundings)]
#![feature(let_chains)]
#![feature(type_alias_impl_trait)]
#![feature(unboxed_closures)]

pub mod algebraic;
pub mod altered_game;
pub mod board;
pub mod chalk;
pub mod chat;
pub mod client;
pub mod client_chat;
pub mod clock;
pub mod coord;
pub mod dirty;
pub mod display;
pub mod error;
pub mod event;
pub mod fen;
pub mod force;
pub mod game;
pub mod grid;
pub mod half_integer;
pub mod iterable_mut;
pub mod janitor;
pub mod lobby;
pub mod meter;
pub mod nanable;
pub mod pgn;
pub mod piece;
pub mod ping_pong;
pub mod player;
pub mod role;
pub mod rules;
pub mod scores;
pub mod server;
pub mod server_chat;
pub mod server_helpers;
pub mod server_hooks;
pub mod session;
pub mod session_store;
pub mod starter;
pub mod test_util;
pub mod utc_time;
pub mod util;
