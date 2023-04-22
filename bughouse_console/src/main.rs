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

mod async_server_main;
mod auth;
mod auth_handlers_tide;
mod censor;
mod client_main;
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
mod stats_handlers_tide;
mod stress_test;

use std::io;

use clap::{arg, Command};
use server_config::{DatabaseOptions, ServerConfig, StringSource};

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
                .arg(arg!(--"secret-sqlite-db" [DB] "Path to the secret sqlite database file"))
                .arg(arg!(--"secret-postgres-db" [DB] "Address of the secret postgres database"))
                .arg(arg!(--"auth" [AUTH_OPTION] "Either NoAuth or Google. The latter enables authentication using Google OAuth2. Reads GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET env variables"))
                .arg(arg!(--"auth-callback-is-https" "Upgrade the auth callback address to https. This can be useful when the server is behind a https proxy such as Apache."))
                .arg(arg!(--"enable-sessions" "Whether to enable the tide session middleware."))
                .arg(arg!(--"static-content-url-prefix" [URL_PREFIX] "Prefix of URLs where static content is served. Generated links referencing static content are based on this URL"))
                .arg(arg!(--"allowed-origin" [ORIGIN] "Allowed origin for websocket requests or * for skipping the check. Only use * for testing")
                    .default_value("https://bughouse.pro"))
                .arg(arg!(--"config-file" [FILE] "Path to the configuration file in yaml-serialized ServerConfig. When specified, other flags are ignored"))
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
            async_server_main::run(server_config_from_args(sub_matches));
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

fn database_options_from_args(args: &clap::ArgMatches) -> DatabaseOptions {
    database_options(args.get_one::<String>("sqlite-db"), args.get_one::<String>("postgres-db"))
}

fn secret_database_options_from_args(args: &clap::ArgMatches) -> DatabaseOptions {
    database_options(
        args.get_one::<String>("secret-sqlite-db"),
        args.get_one::<String>("secret-postgres-db"),
    )
}

fn database_options(sqlite: Option<&String>, postgres: Option<&String>) -> DatabaseOptions {
    match (sqlite, postgres) {
        (None, None) => DatabaseOptions::NoDatabase,
        (Some(_), Some(_)) => panic!("Sqlite and postgres can not be specified simultanously."),
        (Some(db), None) => DatabaseOptions::Sqlite(db.clone()),
        (None, Some(db)) => DatabaseOptions::Postgres(db.clone()),
    }
}

fn read_config_file(filename: &String) -> ServerConfig {
    let contents = std::fs::read_to_string(filename).expect("Reading config file");
    serde_yaml::from_str(&contents).expect("Parsing config file")
}

fn server_config_from_args(args: &clap::ArgMatches) -> ServerConfig {
    if let Some(filename) = args.get_one("config-file") {
        return read_config_file(filename);
    }
    let auth_options = match args.get_one::<String>("auth").map(String::as_str) {
        None | Some("NoAuth") => server_config::AuthOptions::NoAuth,
        Some("Google") => server_config::AuthOptions::Google {
            callback_is_https: args.get_flag("auth-callback-is-https"),
            client_id_source: StringSource::EnvVar("GOOGLE_CLIENT_ID".to_owned()),
            client_secret_source: StringSource::EnvVar("GOOGLE_CLIENT_SECRET".to_owned()),
        },
        Some(a) => panic!("Unrecognized auth option {a}"),
    };
    let session_options = if args.get_flag("enable-sessions") {
        crate::server_config::SessionOptions::WithSessions {
            // The recommended length is at least 512 bits.
            // Accounting for only sampling ascii, 512 / 7 is the minimum.
            secret: StringSource::Random { len: 128 },
            expire_in: time::Duration::days(30).try_into().unwrap(),
        }
    } else {
        crate::server_config::SessionOptions::NoSessions
    };

    let allowed_origin = match args.get_one::<String>("allowed-origin").map(String::as_str) {
        None => panic!("--allowed-origin must be specified"),
        Some("*") => crate::server_config::AllowedOrigin::Any,
        Some(o) => crate::server_config::AllowedOrigin::ThisSite(o.to_owned()),
    };

    let static_content_url_prefix = args
        .get_one::<String>("static-content-url-prefix")
        .cloned()
        .unwrap_or(String::new());
    ServerConfig {
        database_options: database_options_from_args(args),
        secret_database_options: secret_database_options_from_args(args),
        auth_options,
        session_options,
        static_content_url_prefix,
        allowed_origin,
    }
}
