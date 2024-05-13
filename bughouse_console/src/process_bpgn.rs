use std::io::{self, Read};

use bughouse_chess::pgn::{self, BpgnExportFormat, BpgnTimeFormat};
use bughouse_chess::role::Role;


pub struct ProcessBpgnConfig {
    pub role: Role,
    pub remove_timestamps: bool,
}

pub fn run(config: ProcessBpgnConfig) -> io::Result<()> {
    let mut bpgn_in = String::new();
    io::stdin().read_to_string(&mut bpgn_in)?;
    let (game, meta) = match pgn::import_from_bpgn(&bpgn_in, config.role) {
        Ok(game) => game,
        Err(err) => {
            eprintln!("Error reading BPGN: {}", err);
            return Err(io::Error::new(io::ErrorKind::InvalidData, err));
        }
    };
    let time_format = if config.remove_timestamps {
        BpgnTimeFormat::NoTime
    } else {
        BpgnTimeFormat::Timestamp
    };
    let format = BpgnExportFormat { time_format };
    let bpgn_out = pgn::export_to_bpgn(format, &game, meta);
    print!("{bpgn_out}");
    Ok(())
}
