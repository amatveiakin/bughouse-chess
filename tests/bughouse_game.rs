use std::rc::Rc;

use enum_map::{EnumMap, enum_map};
use lazy_static::lazy_static;
use regex::Regex;

use bughouse_chess::{
    ChessRules, BughouseRules, BughouseBoard, BughouseGame,
    GameInstant, TurnMode, TurnError, PlayerInGame, Team, Force
};


fn players() -> EnumMap<BughouseBoard, EnumMap<Force, Rc<PlayerInGame>>> {
    enum_map! {
        BughouseBoard::A => enum_map! {
            Force::White => Rc::new(PlayerInGame{ name: "Alice".to_owned(), team: Team::Red }),
            Force::Black => Rc::new(PlayerInGame{ name: "Bob".to_owned(), team: Team::Blue }),
        },
        BughouseBoard::B => enum_map! {
            Force::White => Rc::new(PlayerInGame{ name: "Charlie".to_owned(), team: Team::Blue }),
            Force::Black => Rc::new(PlayerInGame{ name: "Dave".to_owned(), team: Team::Red }),
        }
    }
}

// Improvement potential: Allow whitespace after turn number.
fn replay_log(game: &mut BughouseGame, log: &str) -> Result<(), TurnError> {
    lazy_static! {
        static ref TURN_NUMBER_RE: Regex = Regex::new(r"^(?:[0-9]+([AaBb])\.)?(.*)$").unwrap();
    }
    let now = GameInstant::game_start();
    for turn_notation in log.split_whitespace() {
        use BughouseBoard::*;
        use Force::*;
        let captures = TURN_NUMBER_RE.captures(turn_notation).unwrap();
        let player_notation = captures.get(1).unwrap().as_str();
        let turn_notation = captures.get(2).unwrap().as_str();
        let (board_idx, force) = match player_notation {
            "A" => (A, White),
            "a" => (A, Black),
            "B" => (B, White),
            "b" => (B, Black),
            _ => panic!("Unexpected bughouse player notation: {}", player_notation),
        };
        assert_eq!(game.board(board_idx).active_force(), force);
        game.try_turn_algebraic(board_idx, turn_notation, TurnMode::Normal, now)?;
    }
    Ok(())
}

#[test]
fn no_castling_with_dropped_rook() {
    let mut game = BughouseGame::new(ChessRules::classic_blitz(), BughouseRules::chess_com(), players());
    replay_log(&mut game, "
        0A.g4  0a.h5
        0A.xh5  0a.Rxh5
        0A.Nf3  0a.Rxh2
        0A.Ng5  0a.Rxh1
        0A.Ne4  0a.Rg1
        0A.Ng3  0a.Rxf1
        0A.Nxf1  0a.e5
        0B.b4  0b.a5
        0B.xa5  0b.Rxa5
        0B.Nf3  0b.Rxa2
        0B.Nc3  0b.Rxa1
        0A.R@h8  0a.d5
    ").unwrap();
    assert_eq!(
        game.try_turn_algebraic(BughouseBoard::A, "0-0", TurnMode::Normal, GameInstant::game_start()).err().unwrap(),
        TurnError::CastlingPieceHasMoved
    );
}
