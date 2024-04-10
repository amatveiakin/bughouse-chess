// Legend for various fix-this comments:
//   * "TODO" - bug or missing crucial feature.
//   * "Improvement potential" - missing nice-to-have feature or an opportunity
//       to make code better or faster.
//   * "Rust-upgrade" - place where code can be improved using a Rust feature
//       that is not implemented or stabilized yet.

#![forbid(unsafe_code)]
#![cfg_attr(feature = "strict", deny(warnings))]

extern crate clap;
extern crate console;
extern crate crossterm;
extern crate enum_map;
extern crate instant;
extern crate itertools;
extern crate scopeguard;
extern crate serde;
extern crate serde_json;
extern crate tungstenite;
extern crate url;

extern crate bughouse_chess;

pub mod network;
pub mod tui;

mod auth;
mod auth_handlers_tide;
mod bughouse_prelude;
mod censor;
mod client_main;
mod client_performance_stats;
mod database;
mod database_server_hooks;
mod game_stats;
mod history_graphs;
mod http_server_state;
mod persistence;
mod prod_server_helpers;
mod secret_database;
mod secret_persistence;
mod server_config;
mod server_main;
mod stats_handlers_tide;
mod stress_test;

use std::io;

use clap::{arg, Command};
use server_config::ServerConfig;

fn main() -> io::Result<()> {
    env_logger::Builder::new()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .filter_module("sqlx::query", log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    let matches = Command::new("Bughouse")
        .author(clap::crate_authors!())
        .version(clap::crate_version!())
        .about("Bughouse chess client/server console app")
        .subcommand_required(true)
        .subcommand(
            Command::new("server")
                .about("Run as server")
                .arg(arg!(<config_file> "Path to the configuration file: yaml-serialized ServerConfig."))
        )
        .subcommand(
            Command::new("client")
                .about("Run as client")
                .arg(arg!(<server_address> "Server address"))
                .arg(arg!(<match_id> "Match ID"))
                .arg(arg!(<player_name> "Player name"))
        )
        .subcommand(
            Command::new("stress-test")
                .about("Stress test different game modes with random input. Can be used for testing or benchmarking.")
                .arg(arg!(<target> "Internal class to test")
                    .value_parser(["pure-game", "altered-game"]))
        )
        .get_matches();

    match matches.subcommand() {
        Some(("server", sub_matches)) => {
            server_main::run(read_config_file(sub_matches.get_one("config_file").unwrap()));
            Ok(())
        }
        Some(("client", sub_matches)) => client_main::run(client_main::ClientConfig {
            server_address: sub_matches.get_one::<String>("server_address").unwrap().clone(),
            match_id: sub_matches.get_one::<String>("match_id").unwrap().clone(),
            player_name: sub_matches.get_one::<String>("player_name").unwrap().clone(),
        }),
        Some(("stress-test", sub_matches)) => stress_test::run(stress_test::StressTestConfig {
            target: sub_matches.get_one::<String>("target").unwrap().clone(),
        }),
        _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }
}

fn read_config_file(filename: &String) -> ServerConfig {
    let contents = std::fs::read_to_string(filename).expect("Reading config file");
    serde_yaml::from_str(&contents).expect("Parsing config file")
}
