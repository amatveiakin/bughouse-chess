// Legend for various fix-this comments:
//   * "TODO" - bug or missing crucial feature.
//   * "Improvement potential" - missing nice-to-have feature or an opportunity
//       to make code better or faster.
//   * "Rust-upgrade" - place where code can be improved using a Rust feature
//       that is not implemented or stabilized yet.

#![forbid(unsafe_code)]

extern crate crossterm;
extern crate clap;
extern crate console;
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

mod async_server_main;
mod auth;
mod auth_handlers_tide;
mod client_main;
mod database;
mod game_stats;
mod history_graphs;
mod http_server_state;
mod persistence;
mod secret_database;
mod secret_persistence;
mod prod_server_helpers;
mod server_main;
mod database_server_hooks;
mod stats_handlers_tide;
mod stress_test;

use std::io;

use clap::{arg, Command};

use server_main::DatabaseOptions;

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
                .arg(arg!(--"sqlite-db" [DB] "Path to an sqlite database file"))
                .arg(arg!(--"postgres-db" [DB] "Address of a postgres database"))
        )
        .subcommand(
            Command::new("async-server")
                .about("Run as server")
                .arg(arg!(--"sqlite-db" [DB] "Path to an sqlite database file"))
                .arg(arg!(--"postgres-db" [DB] "Address of a postgres database"))
                .arg(arg!(--"secret-sqlite-db" [DB] "Path to the secret sqlite database file"))
                .arg(arg!(--"secret-postgres-db" [DB] "Address of the secret postgres database"))
                .arg(arg!(--"auth" [AUTH_OPTION] "Either NoAuth or Google. The latter enables authentication using Google OAuth2. Reads GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET env variables"))
                .arg(arg!(--"auth-callback-is-https" "Upgrade the auth callback address to https. This can be useful when the server is behind a https proxy such as Apache."))
                .arg(arg!(--"enable-sessions" "Whether to enable the tide session middleware."))
                .arg(arg!(--"static-content-url-prefix" [URL_PREFIX] "Prefix of URLs where static content is served. Generated links referencing static content are based on this URL"))
        )
        .subcommand(
            Command::new("client")
                .about("Run as client")
                .arg(arg!(<server_address> "Server address"))
                .arg(arg!(<contest_id> "Contest ID"))
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
            server_main::run(server_main::ServerConfig {
                database_options: database_options_from_args(sub_matches),
                secret_database_options: DatabaseOptions::NoDatabase,
                auth_options: server_main::AuthOptions::NoAuth,
                session_options: server_main::SessionOptions::NoSessions,
                static_content_url_prefix: String::new(),
            });
            Ok(())
        }
        Some(("async-server", sub_matches)) => {
            let auth_options = match sub_matches.get_one::<String>("auth").map(String::as_str) {
                None | Some("NoAuth") => server_main::AuthOptions::NoAuth,
                Some("Google") => server_main::AuthOptions::GoogleAuthFromEnv {
                    callback_is_https: sub_matches.get_flag("auth-callback-is-https"),
                },
                Some(a) => panic!("Unrecognized auth option {a}"),
            };
            let session_options = if sub_matches.get_flag("enable-sessions") {
                crate::server_main::SessionOptions::WithNewRandomSecret
            } else {
                crate::server_main::SessionOptions::NoSessions
            };
            let static_content_url_prefix = sub_matches
                .get_one::<String>("static-content-url-prefix")
                .cloned()
                .unwrap_or(String::new());
            async_server_main::run(server_main::ServerConfig {
                database_options: database_options_from_args(sub_matches),
                secret_database_options: secret_database_options_from_args(sub_matches),
                auth_options,
                session_options,
                static_content_url_prefix,
            });
            Ok(())
        }
        Some(("client", sub_matches)) => {
            client_main::run(client_main::ClientConfig {
                server_address: sub_matches.get_one::<String>("server_address").unwrap().clone(),
                contest_id: sub_matches.get_one::<String>("contest_id").unwrap().clone(),
                player_name: sub_matches.get_one::<String>("player_name").unwrap().clone(),
            })
        }
        Some(("stress-test", sub_matches)) => {
            stress_test::run(stress_test::StressTestConfig {
                target: sub_matches.get_one::<String>("target").unwrap().clone(),
            })
        },
        _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }
}

fn database_options_from_args(args: &clap::ArgMatches) -> DatabaseOptions {
    database_options(
        args.get_one::<String>("sqlite-db"),
        args.get_one::<String>("postgres-db"))
}

fn secret_database_options_from_args(args: &clap::ArgMatches) -> DatabaseOptions {
    database_options(
        args.get_one::<String>("secret-sqlite-db"),
        args.get_one::<String>("secret-postgres-db"))
}

fn database_options(sqlite: Option<&String>, postgres: Option<&String>) -> DatabaseOptions {
    match (sqlite, postgres) {
        (None, None) => DatabaseOptions::NoDatabase,
        (Some(_), Some(_)) => panic!("Sqlite and postgres can not be specified simultanously."),
        (Some(db), None) => DatabaseOptions::Sqlite(db.clone()),
        (None, Some(db)) => DatabaseOptions::Postgres(db.clone()),
    }
}
