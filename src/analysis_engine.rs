// TODO: Make site more responsive when the engine is on. Right now it visibly lags.

use std::mem;
use std::time::Duration;

use crate::algebraic::AlgebraicCharset;
use crate::board::{PromotionTarget, Turn, TurnDrop, TurnInput, TurnMode, TurnMove};
use crate::clock::GameInstant;
use crate::coord::Coord;
use crate::display::DisplayBoard;
use crate::force::Force;
use crate::game::{BughouseBoard, BughouseGame, BughouseGameStatus};
use crate::piece::{CastleDirection, PieceKind};
use crate::rules::{
    ChessRules, DropAggression, FairyPieces, PawnDropRanks, Promotion, Rules, StartingPosition,
};
use crate::{fen, once_cell_regex};


// TODO: Allow analysing both boards.
pub const ANALYSIS_BOARD_IDX: DisplayBoard = DisplayBoard::Primary;
// Improvement potential. Allow users to configure analysis depth.
pub const ANALYSIS_ENGINE_SEARCH_TIME: Duration = Duration::from_secs(5);

// Can be used in lieu of a player name.
pub const ANALYSIS_ENGINE_NAME_GENERIC: &str = "#engine";
pub const ANALYSIS_ENGINE_NAME_WHITE: &str = "#engine-white"; // when playing for White
pub const ANALYSIS_ENGINE_NAME_BLACK: &str = "#engine-black"; // when playing for Black

// General engine status, regardless of whether a position is being analysed at the moment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineStatus {
    NotLoaded,
    AwaitingRules,
    IncompatibleRules,
    Ready,
}

// Positive numbers: White is better. Negative numbers: Black is better.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnalysisScore {
    Centipawn(i32),
    MateIn(i32),        // != 0
    MateDelivered(i32), // -1 or 1
}

#[derive(Clone, Debug)]
pub struct AnalysisInfo {
    pub score: AnalysisScore,
    // `TurnMode::InOrder` are turns that can definitely be executed.
    // `TurnMode::Virtual` are turns that involve virtual piece drops or follow such turns.
    // `TurnMode::Preturn` is not used.
    // All `TurnMode::Virtual` turns always follow `TurnMode::InOrder` turns.
    pub best_line: Vec<(TurnMode, Turn, String)>,
}

#[derive(Clone, Debug)]
struct AnalysisRequest {
    fen: String,
}

#[derive(Clone, Debug, Default)]
enum AnalysisState {
    // No analysis in progress (analysis finished or not requested).
    #[default]
    Idle,
    // Analysis is in progress and the results we are getting from the engine are relevant to the
    // current position.
    Active,
    // We requested a stop, but it hasn't been confirmed yet, so the engine is sending messages
    // related to an obsolete position. May contain a queued request that will be executed when the
    // engine confirms the stop.
    Stopping(Option<AnalysisRequest>),
}

pub trait AnalysisEngine {
    fn status(&self) -> EngineStatus;
    fn stop(&mut self);
    fn new_match(&mut self, rules: &Rules);
    fn new_game(&mut self);
    fn analyze_position(&mut self, game: &BughouseGame, board_idx: BughouseBoard);
    fn process_message(
        &mut self, line: &str, game: &BughouseGame, board_idx: BughouseBoard,
    ) -> Option<AnalysisInfo>;
}

// Fairy Stockfish
pub struct FsfAnalysisEngine {
    status: EngineStatus,
    post_message: Box<dyn Fn(&str)>,
    analysis_state: AnalysisState,
}

impl AnalysisScore {
    pub fn to_percent_score(self) -> f64 {
        match self {
            AnalysisScore::Centipawn(cp) => sigmoid(cp as f64 / 1000.0) * 100.0,
            AnalysisScore::MateIn(..0) => 0.0,
            AnalysisScore::MateIn(0) => panic!(),
            AnalysisScore::MateIn(1..) => 100.0,
            AnalysisScore::MateDelivered(..0) => 0.0,
            AnalysisScore::MateDelivered(0) => panic!(),
            AnalysisScore::MateDelivered(1..) => 100.0,
        }
    }
}

impl FsfAnalysisEngine {
    pub fn new(post_message: Box<dyn Fn(&str)>) -> Self {
        post_message("uci");
        Self {
            status: EngineStatus::AwaitingRules,
            post_message,
            analysis_state: AnalysisState::Idle,
        }
    }

    fn post_analisys_request(&mut self, request: AnalysisRequest) {
        // The `moves` part is mandatory.
        let moves = "";
        (self.post_message)(&format!("position fen {} moves {}", request.fen, moves));
        // (self.post_message)("d"); // print the position for debugging
        (self.post_message)(&format!("go movetime {}", ANALYSIS_ENGINE_SEARCH_TIME.as_millis()));
    }

    // A typical FSF info message for reference:
    //   > info depth 12 seldepth 17 multipv 1 score cp -115 nodes 451791 nps 412218 hashfull 146
    //   > tbhits 0 time 1096 pv c2c3 e6d7 c1g5 P@g4 f3d2 b8c6 P@d5 c6e5
    fn process_info_message(
        &mut self, line: &str, game: &BughouseGame, board_idx: BughouseBoard,
    ) -> Option<AnalysisInfo> {
        let cp_re = once_cell_regex!(r"\bscore cp (-?[0-9]+)");
        let mate_re = once_cell_regex!(r"\bscore mate (-?[0-9]+)");
        let pv_re = once_cell_regex!(r"\bpv (.*)$");

        match self.analysis_state {
            AnalysisState::Idle => {
                panic!("Was not expecting info messages from Fairy-Stockfish, got: {line}")
            }
            AnalysisState::Active => {}
            AnalysisState::Stopping(_) => {
                // Ignore obsolete messages from the previous request.
                return None;
            }
        }

        let score_sign = match game.board(board_idx).active_force() {
            Force::White => 1,
            Force::Black => -1,
        };
        let score = if let Some(cap) = cp_re.captures(line) {
            AnalysisScore::Centipawn(
                score_sign * cap.get(1).unwrap().as_str().parse::<i32>().unwrap(),
            )
        } else if let Some(cap) = mate_re.captures(line) {
            let mate_in = cap.get(1).unwrap().as_str().parse::<i32>().unwrap();
            if mate_in == 0 {
                AnalysisScore::MateDelivered(-score_sign)
            } else {
                AnalysisScore::MateIn(score_sign * mate_in)
            }
        } else {
            return None;
        };

        let mut best_line = Vec::new();
        let mut turn_mode = TurnMode::InOrder;
        if let Some(cap) = pv_re.captures(line) {
            let mut game = game.clone();
            let time = GameInstant::from_game_duration(game.total_time_elapsed());
            // If game ended by flag or resignation, engine might still suggest moves in the final
            // position, so we should set an active status to avoid getting TurnError::GameOver.
            game.set_status(BughouseGameStatus::Active, time);
            for notation in cap.get(1).unwrap().as_str().split_ascii_whitespace() {
                let turn_input = parse_fsf_algebraic(notation);
                if turn_mode == TurnMode::InOrder {
                    if game.try_turn(board_idx, &turn_input, turn_mode, time).is_err() {
                        turn_mode = TurnMode::Virtual;
                    }
                }
                if turn_mode == TurnMode::Virtual {
                    if let Some(err) = game.try_turn(board_idx, &turn_input, turn_mode, time).err()
                    {
                        panic!("Cannot apply Fairy-Stockfish turn \"{notation}\": {err:?}")
                    }
                }
                let last_turn_expanded = &game.turn_log().last().unwrap().turn_expanded;
                let turn = last_turn_expanded.turn;
                let rewritten_notation = last_turn_expanded
                    .algebraic
                    .format(game.board_shape(), AlgebraicCharset::AuxiliaryUnicode);
                best_line.push((turn_mode, turn, rewritten_notation));
            }
        }
        Some(AnalysisInfo { score, best_line })
    }

    // Quote from https://www.shredderchess.com/download/div/uci.zip:
    //   > the engine has stopped searching and found the move <move> best in this position
    // This is what we are after. We want to detect when the engine has finished thinking about the
    // previous turn, so that we know that the sebsequent "info" messages are relevant to the
    // current turn. There is no valuable information in the "bestmove" messages itself for us: UCI
    // guarantees that
    //   > Directly before that the engine should send a final "info" command with the final search
    //   > information
    // We also know that FSF always includes the best line in the info messages.
    fn process_bestmove_message(&mut self) {
        self.analysis_state = match mem::take(&mut self.analysis_state) {
            AnalysisState::Idle => unreachable!(),
            AnalysisState::Active | AnalysisState::Stopping(None) => AnalysisState::Idle,
            AnalysisState::Stopping(Some(request)) => {
                self.post_analisys_request(request);
                AnalysisState::Active
            }
        };
    }
}

impl AnalysisEngine for FsfAnalysisEngine {
    fn status(&self) -> EngineStatus { self.status }

    fn stop(&mut self) {
        match self.analysis_state {
            AnalysisState::Idle => {}
            AnalysisState::Active => {
                self.analysis_state = AnalysisState::Stopping(None);
            }
            AnalysisState::Stopping(_) => return,
        }
        (self.post_message)("stop");
    }

    fn new_match(&mut self, rules: &Rules) {
        self.stop();
        if let Some((variant, fischer_random)) = get_fsf_variant(&rules.chess_rules) {
            let fischer_random = bool_to_str(fischer_random);
            (self.post_message)(&format!("setoption name UCI_Chess960 value {fischer_random}"));
            (self.post_message)(&format!("setoption name UCI_Variant value {variant}"));
            (self.post_message)("ucinewgame");
            self.status = EngineStatus::Ready;
        } else {
            self.status = EngineStatus::IncompatibleRules;
        }
    }

    fn new_game(&mut self) {
        if self.status != EngineStatus::Ready {
            return;
        }
        self.stop();
        (self.post_message)("ucinewgame");
    }

    fn analyze_position(&mut self, game: &BughouseGame, board_idx: BughouseBoard) {
        if self.status != EngineStatus::Ready {
            return;
        }
        let fen = fen::board_to_shredder_fen(&game.board(board_idx));
        let request = AnalysisRequest { fen };
        self.stop();
        self.analysis_state = match self.analysis_state {
            AnalysisState::Idle => {
                self.post_analisys_request(request);
                AnalysisState::Active
            }
            AnalysisState::Active => unreachable!(),
            AnalysisState::Stopping(_) => {
                // Discard the previous request, if any. We don't save analysis replies anywhere,
                // only display them to the user in real time, so we have no use for obsolete data.
                AnalysisState::Stopping(Some(request))
            }
        }
    }

    fn process_message(
        &mut self, line: &str, game: &BughouseGame, board_idx: BughouseBoard,
    ) -> Option<AnalysisInfo> {
        if line.starts_with("info") {
            self.process_info_message(line, game, board_idx)
        } else if line.starts_with("bestmove") {
            self.process_bestmove_message();
            None
        } else {
            None
        }
    }
}

fn sigmoid(x: f64) -> f64 { 1.0 / (1.0 + (-x).exp()) }

fn bool_to_str(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

// Differences from regular algebraic notation:
//   - Both starting and ending squares are required;
//   - Move piece kind is not specified;
//   - Promotion target does not have a separator and can be denoted by a lower-case letter;
//   - Can castle by moving the kind.
//
// The last part is actually the main reason why this wasn't united with the main algebraic parser.
// Users can input turns via algebraic notation, and I would like to preserve the nice property that
// algebraic commands are executed accurately. In particular, it is valid to premove “Kb1a1” in case
// your opponent captures a rook and expect it not to be resolved as castling if they don't. So if
// we do the unification, we need to add algebraic dialects or something.
fn parse_fsf_algebraic(notation: &str) -> TurnInput {
    let move_re = once_cell_regex!(r"^([a-h][1-8])([a-h][1-8])([a-zA-Z])?$");
    let drop_re = once_cell_regex!(r"^([A-Z])@([a-h][1-8])$");
    const A_CASTLING: &'static str = "O-O-O";
    const H_CASTLING: &'static str = "O-O";
    let turn = if let Some(cap) = move_re.captures(notation) {
        let from = Coord::from_algebraic(cap.get(1).unwrap().as_str()).unwrap();
        let to = Coord::from_algebraic(cap.get(2).unwrap().as_str()).unwrap();
        let promote_to = cap
            .get(3)
            .map(|s| PieceKind::from_algebraic_ignore_case(s.as_str()).unwrap())
            .map(PromotionTarget::Upgrade);
        Turn::Move(TurnMove { from, to, promote_to })
    } else if let Some(cap) = drop_re.captures(notation) {
        let piece_kind = PieceKind::from_algebraic(cap.get(1).unwrap().as_str()).unwrap();
        let to = Coord::from_algebraic(cap.get(2).unwrap().as_str()).unwrap();
        Turn::Drop(TurnDrop { piece_kind, to })
    } else if notation == A_CASTLING {
        Turn::Castle(CastleDirection::ASide)
    } else if notation == H_CASTLING {
        Turn::Castle(CastleDirection::HSide)
    } else {
        panic!("Cannot parse Fairy-Stockfish turn: \"{notation}\"");
    };
    // Use TurnInput::DragDrop to allow castling auto-detection.
    // TODO: Add a separate turn kind or extend algebraic notation to support this.
    TurnInput::DragDrop(turn)
}

fn get_fsf_variant(rules: &ChessRules) -> Option<(&'static str, bool)> {
    // TODO: Use variant config parsing to support more combinations:
    //   https://github.com/fairy-stockfish/Fairy-Stockfish/commit/647853cd9eba78fb3db3d0499c6fc36567253229
    let bughouse_rules = rules.bughouse_rules.as_ref().unwrap();
    if rules.fairy_pieces != FairyPieces::NoFairy
        || rules.duck_chess
        || rules.atomic_chess
        || rules.fog_of_war
        || rules.promotion() != Promotion::Upgrade
        || bughouse_rules.drop_aggression != DropAggression::MateAllowed
        || bughouse_rules.pawn_drop_ranks != PawnDropRanks::from_one_based(2, 7)
    {
        return None;
    }
    let variant = if bughouse_rules.koedem { "koedem" } else { "bughouse" };
    let is_fischer_random = match rules.starting_position {
        StartingPosition::Classic => false,
        StartingPosition::FischerRandom => true,
    };
    Some((variant, is_fischer_random))
}
