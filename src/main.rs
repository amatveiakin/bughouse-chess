extern crate console;
extern crate derive_new;
extern crate enum_map;
extern crate itertools;
extern crate lazy_static;
extern crate rand;
extern crate regex;

pub mod chess;  // TODO: Remove `pub` (it's for unused imports warning)
mod coord;
mod force;
mod grid;
mod janitor;
mod piece;
mod util;

use std::io;

use chess::*;
use coord::*;


fn main() -> io::Result<()> {
    let chess_rules = ChessRules {
        starting_position: StartingPosition::Classic,
    };
    let _bughouse_rules = BughouseRules {
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(7),
        drop_aggression: DropAggression::NoChessMate,
    };

    let mut game = ChessGame::new(chess_rules.clone());
    // game.try_turn_from_algebraic("e4").unwrap();
    // game.try_replay_log("
    //     1.Nf3 Nf6 2.c4 g6 3.Nc3 Bg7 4.d4 O-O 5.Bf4 d5
    //     6.Qb3 dxc4 7.Qxc4 c6 8.e4 Nbd7 9.Rd1 Nb6 10.Qc5 Bg4
    //     11.Bg5 Na4 12.Qa3 Nxc3 13.bxc3 Nxe4 14.Bxe7 Qb6 15.Bc4 Nxc3
    //     16.Bc5 Rfe8+ 17.Kf1 Be6 18.Bxb6 Bxc4+ 19.Kg1 Ne2+ 20.Kf1 Nxd4+
    //     21.Kg1 Ne2+ 22.Kf1 Nc3+ 23.Kg1 axb6 24.Qb4 Ra4 25.Qxb6 Nxd1
    //     26.h3 Rxa2 27.Kh2 Nxf2 28.Re1 Rxe1 29.Qd8+ Bf8 30.Nxe1 Bd5
    //     31.Nf3 Ne4 32.Qb8 b5 33.h4 h5 34.Ne5 Kg7 35.Kg1 Bc5+
    //     36.Kf1 Ng3+ 37.Ke1 Bb4+ 38.Kd1 Bb3+ 39.Kc1 Ne2+ 40.Kb1 Nc3+
    //     41.Kc1 Rc2#
    // ").unwrap();
    println!("{}\n", game.render_as_unicode());
    loop {
        let mut buffer = String::new();
        let stdin = io::stdin();
        stdin.read_line(&mut buffer)?;
        if let Err(e) = game.try_replay_log(&buffer) {
            println!("Impossible move: {:?}", e);
        } else {
            println!("{}\n", game.render_as_unicode());
        }
        if game.status() != GameStatus::Active {
            println!("\n{:?}", game.status());
            return Ok(());
        }
    }

    // let mut game = BughouseGame::new(chess_rules, bughouse_rules);
    // game.try_turn(0, Turn::Move(TurnMove{ from: Coord::E2, to: Coord::E4, promote_to: None })).unwrap();
}
