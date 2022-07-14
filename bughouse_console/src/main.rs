// Legend for various fix-this comments:
//   * "TODO" - bug or missing crucial feature.
//   * "Improvement potential" - missing nice-to-have feature or an opportunity
//       to make code better or faster.
//   * "Rust-upgrade" - place where code can be improved using a Rust feature
//       that is not implemented or stabilized yet.

extern crate crossterm;
extern crate clap;
extern crate console;
extern crate enum_map;
extern crate instant;
extern crate itertools;
extern crate regex;
extern crate scopeguard;
extern crate serde;
extern crate serde_json;
extern crate tungstenite;
extern crate url;

extern crate bughouse_chess;

pub mod network;
pub mod tui;

mod client_main;
mod server_main;

use std::io;

use clap::{arg, Command};


fn main() -> io::Result<()> {
    let matches = Command::new("Bughouse")
        .author(clap::crate_authors!())
        .version(clap::crate_version!())
        .about("Bughouse chess client/server console app")
        .subcommand_required(true)
        .subcommand(
            Command::new("server")
                .about("Run as server")
                .arg(arg!(--starting_time [TIME] "Starting time for each player")
                    .default_value("5:00"))
                .arg(arg!(--teaming [TEAMING] "How players are split into teams")
                    .possible_values(["fixed", "dynamic"]).default_value("fixed"))
        )
        .subcommand(
            Command::new("client")
                .about("Run as client")
                .arg(arg!(<server_address> "Server address"))
                .arg(arg!(<player_name> "Player name"))
                .arg(arg!([team] "Team").possible_values(["red", "blue"]))
        )
        .get_matches();

    match matches.subcommand() {
        Some(("server", sub_matches)) => {
            server_main::run(server_main::ServerConfig {
                teaming: sub_matches.value_of("teaming").unwrap().to_string(),
                starting_time: sub_matches.value_of("starting_time").unwrap().to_string(),
            });
            Ok(())
        },
        Some(("client", sub_matches)) => {
            client_main::run(client_main::ClientConfig {
                server_address: sub_matches.value_of("server_address").unwrap().to_string(),
                player_name: sub_matches.value_of("player_name").unwrap().to_string(),
                team: sub_matches.value_of("team").map(|s| s.to_string()),
            })
        },
        _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }
}
