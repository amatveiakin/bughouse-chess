use std::io;

use clap::{arg, Command};

mod client_main;
mod server_main;


fn main() -> io::Result<()> {
    let matches = Command::new("Bughouse")
        .author(clap::crate_authors!())
        .version(clap::crate_version!())
        .about("Bughouse chess client/server console app")
        .subcommand_required(true)
        .subcommand(
            Command::new("server")
                .about("Run as server")
        )
        .subcommand(
            Command::new("client")
                .about("Run as client")
                .arg(arg!(<server_address> "Server address"))
                .arg(arg!(<player_name> "Player name"))
                .arg(arg!(<team> "Team").possible_values(["red", "blue"]))
        )
        .get_matches();

    match matches.subcommand() {
        Some(("server", _)) => {
            server_main::run();
            Ok(())
        },
        Some(("client", sub_matches)) => {
            client_main::run(client_main::ClientConfig {
                server_address: sub_matches.value_of("server_address").unwrap().to_string(),
                player_name: sub_matches.value_of("player_name").unwrap().to_string(),
                team: sub_matches.value_of("team").unwrap().to_string(),
            })
        },
        _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }
}
