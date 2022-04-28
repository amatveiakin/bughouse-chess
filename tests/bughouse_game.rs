use std::rc::Rc;

use enum_map::{EnumMap, enum_map};

use bughouse_chess::{
    ChessRules, BughouseRules, BughouseBoard, BughouseGame,
    GameInstant, TurnError, Player, Team, Force
};


fn players() -> EnumMap<BughouseBoard, EnumMap<Force, Rc<Player>>> {
    enum_map! {
        BughouseBoard::A => enum_map! {
            Force::White => Rc::new(Player{ name: "Alice".to_owned(), team: Team::Red }),
            Force::Black => Rc::new(Player{ name: "Bob".to_owned(), team: Team::Blue }),
        },
        BughouseBoard::B => enum_map! {
            Force::White => Rc::new(Player{ name: "Charlie".to_owned(), team: Team::Blue }),
            Force::Black => Rc::new(Player{ name: "Dave".to_owned(), team: Team::Red }),
        }
    }
}

#[test]
fn no_castling_with_dropped_rook() {
    let mut game = BughouseGame::new(ChessRules::classic_blitz(), BughouseRules::chess_com(), players());
    game.TEST_try_replay_log("
        0A.g4  0a.h5
        0A.xh5  0a.Rxh5
        0A.Nf3  0a.Rxh2
        0A.Ng5  0a.Rxh1
        0A.Ne4  0a.Rxg1
        0A.Ng3  0a.Rxf1
        0A.Nxf1  0a.e5
        0B.b4  0b.a5
        0B.xa5  0b.Rxa5
        0B.Nf3  0b.Rxa2
        0B.Nc3  0b.Rxa1
        0A.R@h8  0a.d5
    ").unwrap();
    assert_eq!(
        game.try_turn_from_algebraic(BughouseBoard::A, "0-0", GameInstant::game_start()).err().unwrap(),
        TurnError::CastlingPieceHasMoved
    );
}
