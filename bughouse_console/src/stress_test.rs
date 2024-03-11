// Improvement potential: Add analog of bughouse_online.rs test. Connect multiple clients
// to a virtual server and execute random actions on the clients. Verify that the server
// and the clients do not panic.

use std::cell::RefCell;
use std::time::Duration;
use std::{io, panic};

use bughouse_chess::role::Role;
use bughouse_chess::test_util::*;
use instant::Instant;
use rand::distributions::WeightedIndex;
use rand::prelude::*;

use crate::bughouse_prelude::*;


const PURE_GAMES_PER_BATCH: usize = 100;
const ALTERED_GAMES_PER_BATCH: usize = 10;
const TURNS_PER_GAME: usize = 100_000;
const ACTIONS_PER_GAME: usize = 10_000;
const MAX_ATTEMPTS_GENERATING_SERVER_TURN: usize = 10_000;
const DROP_RATIO: f64 = 0.2;
const DRAG_RESERVE_RATIO: f64 = 0.3;
const PROMOTION_RATIO: f64 = 0.2;
const QUIT_INACTIVE_GAME_RATIO: f64 = 0.1;

pub struct StressTestConfig {
    pub target: String,
}

#[derive(Clone, Copy, Debug)]
enum ActionKind {
    SetStatus,
    ApplyRemoteTurn,
    LocalTurn,
    PieceDragState,
    StartDragPiece,
    AbortDragPiece,
    DragPieceDrop,
    CancelPreturn,
}

// Improvement potential. Find or implement a way to automatically generate such a enum and apply it.
#[derive(Clone, Debug)]
enum Action {
    SetStatus {
        status: BughouseGameStatus,
        time: GameInstant,
    },
    ApplyRemoteTurn {
        envoy: BughouseEnvoy,
        turn_input: TurnInput,
        time: GameInstant,
    },
    LocalTurn {
        board_idx: BughouseBoard,
        turn_input: TurnInput,
        time: GameInstant,
    },
    PieceDragState,
    StartDragPiece {
        board_idx: BughouseBoard,
        start: PieceDragStart,
    },
    AbortDragPiece,
    DragPieceDrop {
        board_idx: BughouseBoard,
        dest: Coord,
    },
    CancelPreturn {
        board_idx: BughouseBoard,
    },
}

#[derive(Default)]
struct TestState {
    alt_game: Option<AlteredGame>,
    last_action: Option<Action>,
}

thread_local! {
    static TEST_STATE: RefCell<TestState> = RefCell::new(TestState::default());
}

fn random_rules(rng: &mut rand::rngs::ThreadRng) -> Rules {
    loop {
        let rules = Rules {
            match_rules: MatchRules::unrated(),
            chess_rules: ChessRules {
                fairy_pieces: if rng.gen::<bool>() {
                    FairyPieces::NoFairy
                } else {
                    FairyPieces::Accolade
                },
                starting_position: if rng.gen::<bool>() {
                    StartingPosition::Classic
                } else {
                    StartingPosition::FischerRandom
                },
                duck_chess: rng.gen::<bool>(),
                atomic_chess: rng.gen::<bool>(),
                fog_of_war: rng.gen::<bool>(),
                time_control: TimeControl { starting_time: Duration::from_secs(300) },
                bughouse_rules: Some(BughouseRules {
                    koedem: rng.gen::<bool>(),
                    // Improvement potential: Test other promotion strategies.
                    promotion: Promotion::Upgrade,
                    // Improvement potential: Wider range of pawn drop ranks for larger boards.
                    pawn_drop_ranks: PawnDropRanks {
                        min: SubjectiveRow::from_one_based(rng.gen_range(1..=7)),
                        max: SubjectiveRow::from_one_based(rng.gen_range(1..=7)),
                    },
                    drop_aggression: match rng.gen_range(0..4) {
                        0 => DropAggression::NoCheck,
                        1 => DropAggression::NoChessMate,
                        2 => DropAggression::NoBughouseMate,
                        3 => DropAggression::MateAllowed,
                        _ => unreachable!(),
                    },
                }),
            },
        };
        if rules.verify().is_ok() {
            return rules;
        }
    }
}

fn bughouse_game(rules: Rules) -> BughouseGame {
    BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players())
}

fn random_coord(rng: &mut rand::rngs::ThreadRng, board_shape: BoardShape) -> Coord {
    Coord::new(
        Row::from_zero_based(rng.gen_range(0..board_shape.num_rows as i8)),
        Col::from_zero_based(rng.gen_range(0..board_shape.num_cols as i8)),
    )
}

fn random_piece(rng: &mut rand::rngs::ThreadRng) -> PieceKind {
    use PieceKind::*;
    let pieces = [Pawn, Knight, Bishop, Rook, Queen, King];
    *pieces.choose(rng).unwrap()
}

fn random_force(rng: &mut rand::rngs::ThreadRng) -> Force {
    if rng.gen::<bool>() {
        Force::White
    } else {
        Force::Black
    }
}

fn random_board(rng: &mut rand::rngs::ThreadRng) -> BughouseBoard {
    if rng.gen::<bool>() {
        BughouseBoard::A
    } else {
        BughouseBoard::B
    }
}

fn random_turn(rng: &mut rand::rngs::ThreadRng, board_shape: BoardShape) -> Turn {
    // Note: Castling is also covered thanks to `TurnInput::DragDrop`.
    if rng.gen_bool(DROP_RATIO) {
        Turn::Drop(TurnDrop {
            to: random_coord(rng, board_shape),
            piece_kind: random_piece(rng),
        })
    } else {
        // Trying to strike balance: on the one hand, we want to include invalid turns.
        // On the other hand, too many invalid turns would mean that very little happens.
        // Decision: randomly try to promote all pieces (pawns and non-pawns), but only
        // if they are potentially on the last row.
        let from = random_coord(rng, board_shape);
        let to = random_coord(rng, board_shape);

        let promote_to = if to.row == Row::_1 || to.row == Row::_8 && rng.gen_bool(PROMOTION_RATIO)
        {
            Some(PromotionTarget::Upgrade(random_piece(rng)))
        } else {
            None
        };
        Turn::Move(TurnMove { from, to, promote_to })
    }
}

fn random_action_kind(rng: &mut rand::rngs::ThreadRng) -> ActionKind {
    let n = 10_000_000;
    assert!(n >= TURNS_PER_GAME * 10);
    use ActionKind::*;
    let weighted_actions = [
        (SetStatus, n / TURNS_PER_GAME / 2), // these end the game, so should only have very few
        (ApplyRemoteTurn, n / 10),           // these are always valid (and expensive)
        (LocalTurn, n),
        (PieceDragState, n),
        (StartDragPiece, n),
        (AbortDragPiece, n),
        (DragPieceDrop, n),
        (CancelPreturn, n),
    ];
    let (actions, weights): (Vec<_>, Vec<_>) = weighted_actions.into_iter().unzip();
    let dist = WeightedIndex::new(weights).unwrap();
    actions[dist.sample(rng)]
}

fn random_action(alt_game: &AlteredGame, rng: &mut rand::rngs::ThreadRng) -> Option<Action> {
    use ActionKind::*;
    let board_shape = alt_game.chess_rules().board_shape();
    Some(match random_action_kind(rng) {
        SetStatus => {
            let status = BughouseGameStatus::Victory(Team::Red, VictoryReason::Resignation);
            let time = GameInstant::game_start();
            Action::SetStatus { status, time }
        }
        ApplyRemoteTurn => {
            // Optimization potential. A more direct way of generating random valid moves.
            for _ in 0..MAX_ATTEMPTS_GENERATING_SERVER_TURN {
                let mut game = alt_game.game_confirmed().clone();
                let mut random_envoy = || {
                    let board_idx = random_board(rng);
                    let force = game.board(board_idx).active_force();
                    BughouseEnvoy { board_idx, force }
                };
                let mut envoy = random_envoy();
                // The client can reasonably expect that the server wouldn't send back turns by
                // the current player other than those which they actually made (except while
                // reconnecting). Some assertions along those lines are sprinkled here and there
                // in AlteredGame. But we don't want to track too much state in this test. This
                // would be a step away from fuzzing towards traditional integration testing,
                // which is not the focus here. So here's a, ahem, solution: never confirm any
                // turns by the current player! This limits the scope of the test, of course,
                // but I don't have better ideas for now. Perhaps AlteredGame is just a bad
                // layer of abstraction for fuzzing, and this test should be deleted when a
                // client/server fuzzer test is in place.
                // Note that as a consequence we cannot have double-play.
                // TODO: Fix this and test double-play scenarios as well.
                while alt_game.my_id().plays_for(envoy) {
                    envoy = random_envoy();
                }
                let turn = random_turn(rng, board_shape);
                let turn_is_valid = game
                    .try_turn(
                        envoy.board_idx,
                        &TurnInput::DragDrop(turn),
                        TurnMode::Normal,
                        GameInstant::game_start(),
                    )
                    .is_ok();
                if turn_is_valid {
                    let turn_algebraic = game
                        .last_turn_record()
                        .unwrap()
                        .turn_expanded
                        .algebraic
                        .format(game.board_shape(), AlgebraicCharset::Ascii);
                    let turn_input = TurnInput::Algebraic(turn_algebraic);
                    let time = GameInstant::game_start();
                    return Some(Action::ApplyRemoteTurn { envoy, turn_input, time });
                }
            }
            return None;
        }
        LocalTurn => {
            let board_idx = random_board(rng);
            let turn_input = TurnInput::DragDrop(random_turn(rng, board_shape));
            let time = GameInstant::game_start();
            Action::LocalTurn { board_idx, turn_input, time }
        }
        PieceDragState => Action::PieceDragState,
        StartDragPiece => {
            let board_idx = random_board(rng);
            let start = if rng.gen_bool(DRAG_RESERVE_RATIO) {
                PieceDragStart::Reserve(random_piece(rng))
            } else {
                PieceDragStart::Board(random_coord(rng, board_shape))
            };
            Action::StartDragPiece { board_idx, start }
        }
        AbortDragPiece => Action::AbortDragPiece,
        DragPieceDrop => {
            let board_idx = random_board(rng);
            let dest = random_coord(rng, board_shape);
            Action::DragPieceDrop { board_idx, dest }
        }
        CancelPreturn => {
            let board_idx = random_board(rng);
            Action::CancelPreturn { board_idx }
        }
    })
}

#[allow(clippy::let_unit_value)]
fn apply_action(alt_game: &mut AlteredGame, action: Action) {
    use Action::*;
    match action {
        SetStatus { status, time } => _ = alt_game.set_status(status, time),
        ApplyRemoteTurn { envoy, turn_input, time } => {
            _ = alt_game.apply_remote_turn(envoy, &turn_input, time)
        }
        LocalTurn { board_idx, turn_input, time } => {
            _ = alt_game.try_local_turn(board_idx, turn_input, time)
        }
        PieceDragState => _ = alt_game.piece_drag_state(),
        StartDragPiece { board_idx, start } => _ = alt_game.start_drag_piece(board_idx, start),
        AbortDragPiece => _ = alt_game.abort_drag_piece(),
        DragPieceDrop { board_idx, dest } => _ = alt_game.drag_piece_drop(board_idx, dest),
        CancelPreturn { board_idx } => _ = alt_game.cancel_preturn(board_idx),
    }
}


pub fn bughouse_game_test() -> io::Result<()> {
    let rng = &mut rand::thread_rng();
    loop {
        let t0 = Instant::now();
        let mut finished_games = 0;
        let mut total_turns = 0;
        let mut successful_turns = 0;
        for _ in 0..PURE_GAMES_PER_BATCH {
            let mut game = bughouse_game(random_rules(rng));
            for _ in 0..TURNS_PER_GAME {
                let ret = game.try_turn(
                    random_board(rng),
                    &TurnInput::DragDrop(random_turn(rng, game.chess_rules().board_shape())),
                    TurnMode::Normal,
                    GameInstant::game_start(),
                );
                total_turns += 1;
                if ret.is_ok() {
                    successful_turns += 1;
                }
                if !game.is_active() && rng.gen_bool(QUIT_INACTIVE_GAME_RATIO) {
                    break;
                }
            }
            if !game.is_active() {
                finished_games += 1;
            }
        }
        let elpased = t0.elapsed();
        println!(
            "Ran: {} games ({} finished), {} turns ({} successful) in {:.2}s",
            PURE_GAMES_PER_BATCH,
            finished_games,
            total_turns,
            successful_turns,
            elpased.as_secs_f64(),
        );
    }
}

pub fn altered_game_test() -> io::Result<()> {
    let std_panic_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        TEST_STATE.with(|cell| {
            if let Some(ref last_action) = cell.borrow().last_action {
                println!("Last action: {last_action:?}");
            }
            if let Some(ref alt_game) = cell.borrow().alt_game {
                println!("AlteredGame before action:\n{alt_game:#?}");
            }
        });
        std_panic_hook(panic_info);
    }));
    let rng = &mut rand::thread_rng();
    loop {
        let t0 = Instant::now();
        let mut finished_games = 0;
        for _ in 0..ALTERED_GAMES_PER_BATCH {
            let participant =
                BughouseParticipant::Player(BughousePlayer::SinglePlayer(BughouseEnvoy {
                    board_idx: random_board(rng),
                    force: random_force(rng),
                }));
            let mut alt_game = AlteredGame::new(participant, bughouse_game(random_rules(rng)));
            for _ in 0..ACTIONS_PER_GAME {
                let Some(action) = random_action(&alt_game, rng) else {
                    continue;
                };
                TEST_STATE.with(|cell| {
                    let state = &mut cell.borrow_mut();
                    state.alt_game = Some(alt_game.clone());
                    state.last_action = Some(action.clone());
                });
                apply_action(&mut alt_game, action);
                alt_game.local_game();
                if !alt_game.is_active() && rng.gen_bool(QUIT_INACTIVE_GAME_RATIO) {
                    break;
                }
            }
            if !alt_game.is_active() {
                finished_games += 1;
            }
        }
        let elpased = t0.elapsed();
        println!(
            "Ran: {} games ({} finished) in {:.2}s",
            ALTERED_GAMES_PER_BATCH,
            finished_games,
            elpased.as_secs_f64(),
        );
    }
}

pub fn run(config: StressTestConfig) -> io::Result<()> {
    match config.target.as_str() {
        "pure-game" => bughouse_game_test(),
        "altered-game" => altered_game_test(),
        _ => panic!("Invalid stress test target: {}", config.target),
    }
}


// Comment the code above and uncomment this to compile without the stress test:
//
// use std::io;
// pub struct StressTestConfig {
//     pub target: String,
// }
// pub fn run(_: StressTestConfig) -> io::Result<()> { todo!() }
