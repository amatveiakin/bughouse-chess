// Legend for various fix-this comments:
//   * "TODO" - bug or missing crucial feature.
//   * "Improvement potential" - missing nice-to-have feature or an opportunity
//       to make code better or faster.
//   * "Rust-upgrade" - place where code can be improved using a Rust feature
//       that is not implemented or stabilized yet.

#![forbid(unsafe_code)]
#![feature(deadline_api)]
#![feature(thread_sleep_until)]
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
mod check_player_name;
mod client_main;
mod client_performance_stats;
mod competitor;
mod database;
mod database_server_hooks;
mod game_stats;
mod history_graphs;
mod http_server_state;
mod load_test;
mod persistence;
mod process_bpgn;
mod prod_server_helpers;
mod secret_database;
mod secret_persistence;
mod server_config;
mod server_main;
mod stats_handlers_tide;
mod stress_test;

use std::io;

use bughouse_chess::role::Role;
use clap::{Command, arg};
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
        .subcommand(Command::new("server").about("Run as server").arg(
            arg!(<config_file> "Path to the configuration file: yaml-serialized ServerConfig."),
        ))
        .subcommand(
            Command::new("client")
                .about("Run as client")
                .arg(arg!(<server_address> "Server address"))
                .arg(arg!(<match_id> "Match ID"))
                .arg(arg!(<player_name> "Player name")),
        )
        .subcommand(
            Command::new("load-test")
                .about("Load test a given server")
                .arg(arg!(<server_address> "Server address"))
                .arg(
                    arg!(-'n' --"matches" <n> "Number of simultaneous games, four clients each")
                        .value_parser(1..=1000)
                        .default_value("100"),
                ),
        )
        .subcommand(
            Command::new("stress-test")
                .about(concat!(
                    "Stress test different game modes with random input. ",
                    "Can be used for testing or benchmarking."
                ))
                .arg(
                    arg!(<target> "Internal class to test")
                        .value_parser(["pure-game", "altered-game"]),
                ),
        )
        .subcommand(
            Command::new("bpgn")
                .about("Reads a BPGN from stdin, transforms it and writes the result to stdout.")
                .arg(
                    arg!(--"role" <role>)
                        .value_parser(["server", "client"])
                        .default_value("server"),
                )
                .arg(arg!(--"remove-timestamps" "Removes turn timestamps and GameDuration tag.")),
        )
        .subcommand(
            Command::new("check-name")
                .about("Verifies whether a player name is valid (but not necessarily free).")
                .arg(arg!(<player_name> "Player name to check")),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("server", sub_matches)) => {
            async_std::task::block_on(server_main::run(read_config_file(
                sub_matches.get_one("config_file").unwrap(),
            )));
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
        Some(("load-test", sub_matches)) => load_test::run(load_test::LoadTestConfig {
            server_address: sub_matches.get_one::<String>("server_address").unwrap().clone(),
            num_matches: *sub_matches.get_one::<i64>("matches").unwrap() as usize,
        }),
        Some(("bpgn", sub_matches)) => process_bpgn::run(process_bpgn::ProcessBpgnConfig {
            role: match sub_matches.get_one::<String>("role").unwrap().as_str() {
                "server" => Role::ServerOrStandalone,
                "client" => Role::Client,
                _ => panic!(),
            },
            remove_timestamps: sub_matches.get_flag("remove-timestamps"),
        }),
        Some(("check-name", sub_matches)) => {
            check_player_name::run(&sub_matches.get_one::<String>("player_name").unwrap().clone())
        }
        _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }
}

fn read_config_file(filename: &String) -> ServerConfig {
    let contents = std::fs::read_to_string(filename).expect("Reading config file");
    serde_yaml::from_str(&contents).expect("Parsing config file")
}
