#![forbid(unsafe_code)]
#![cfg_attr(feature = "strict", deny(warnings))]
#![feature(anonymous_lifetime_in_impl_trait)]
#![feature(async_closure)]
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
pub mod session;
pub mod starter;
pub mod test_util;
pub mod utc_time;
pub mod util;

#[cfg(not(target_arch = "wasm32"))]
pub mod server;
#[cfg(not(target_arch = "wasm32"))]
pub mod server_chat;
#[cfg(not(target_arch = "wasm32"))]
pub mod server_helpers;
#[cfg(not(target_arch = "wasm32"))]
pub mod server_hooks;
#[cfg(not(target_arch = "wasm32"))]
pub mod session_store;
