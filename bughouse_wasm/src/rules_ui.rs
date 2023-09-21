use std::fmt;

use itertools::Itertools;


pub const PLAYER_NAME: &'static str = "player_name"; // filled by JSs
pub const RATING: &'static str = "rating"; // filled by JSs
pub const FAIRY_PIECES: &'static str = "fairy_pieces";
pub const STARTING_POSITION: &'static str = "starting_position";
pub const DUCK_CHESS: &'static str = "duck_chess";
pub const FOG_OF_WAR: &'static str = "fog_of_war";
pub const STARTING_TIME: &'static str = "starting_time";
pub const PROMOTION: &'static str = "promotion";
pub const DROP_AGGRESSION: &'static str = "drop_aggression";
pub const PAWN_DROP_RANKS: &'static str = "pawn_drop_ranks";

pub struct RuleNode {
    name: String,
    label: String,
    input: Option<String>,
    tooltip: Option<String>,
}

impl RuleNode {
    pub fn new(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            input: None,
            tooltip: None,
        }
    }

    pub fn class(&self) -> String { format!("rule-setting-{}", self.name) }

    pub fn with_input_select<S1: fmt::Display, S2: fmt::Display>(
        mut self, options: impl IntoIterator<Item = (S1, S2, bool)>,
    ) -> Self {
        let mut num_selected = 0;
        self.input = Some(format!(
            "<select name='{}' class='{}'>{}</select>",
            self.name,
            self.class(),
            options
                .into_iter()
                .map(|(value, label, selected)| {
                    let mut selected_attr = String::new();
                    if selected {
                        num_selected += 1;
                        selected_attr = "selected".to_string();
                    }
                    format!(
                        "<option value='{}' {}>{}</option>",
                        value.to_string(),
                        selected_attr,
                        html_escape::encode_text(&label.to_string()),
                    )
                })
                .join("")
        ));
        assert_eq!(num_selected, 1);
        self
    }

    pub fn with_input_text(
        mut self, pattern: impl fmt::Display, placeholder: impl fmt::Display,
        value: impl fmt::Display,
    ) -> Self {
        let name = &self.name;
        let class = self.class();
        self.input = Some(format!(
            "<input type='text' name='{name}' class='{class}'
            pattern='{pattern}' placeholder='{placeholder}' value='{value}'
            spellcheck='false' autocomplete='off' required/>"
        ));
        self
    }

    pub fn with_tooltip<S: fmt::Display>(
        mut self, paragraphs: impl IntoIterator<Item = S>,
    ) -> Self {
        self.tooltip = Some(paragraphs.into_iter().map(|p| format!("<p>{}</p>", p)).join(""));
        self
    }

    pub fn to_html(&self) -> String {
        let mut html = String::new();
        html.push_str(&format!(
            "<label for='{}' class='{}'>{}</label>",
            self.name,
            self.class(),
            html_escape::encode_text(&self.label)
        ));
        html.push_str(self.input.as_ref().unwrap());
        if let Some(tooltip) = &self.tooltip {
            html.push_str(&format!(
                "<div class='tooltip-standalone {}'><div class='tooltip-text'>{}</div></div>",
                self.class(),
                tooltip
            ));
        } else {
            html.push_str(&format!("<div class='{}'></div>", self.class()))
        }
        html
    }
}

pub fn accolade_tooltip() -> &'static str {
    "<i>Accolade.</i>
    Confer knighthood to any piece! Combine a Knight with a Bishop, a Rook or a Queen to get
    a Cardinal, an Empress or an Amazon respectively. Combine by moving one piece onto another
    or by dropping one piece onto another. If captured, the piece falls back apart."
}

pub fn fischer_random_tooltip() -> &'static str {
    "<i>Fischer random</i> (a.k.a. Chess960).
    Pawns start as usual. Pieces start on the home ranks, but their
    positions are randomized. Bishops are always of opposite colors. King always starts between
    the rooks. Castling is allowed: A-side castling puts the kind and left rook on files C and&nbspD
    respectively, H-side castling puts the kind and right rook on files G and&nbspF respectively.
    All four players start with the same setup."
}

pub fn duck_chess_tooltip() -> &'static str {
    "<i>Duck chess.</i>
    A duck occupies one square on the board and cannot be captured. Each turn consists of two parts.
    First, a regular bughouse turn. Second, moving the duck to any free square on the board. Quack!"
}

pub fn fog_of_war_tooltip() -> &'static str {
    "<i>Fog of war.</i>
    You only see squares that are a legal move destination for one of your pieces.
    You can drop pieces into the fog of war at your own risk."
}

pub fn stating_time_tooltip() -> &'static str {
    "Starting time in “m:ss” format. Increments and delays are not allowed."
}

pub fn promotion_upgrade_tooltip() -> &'static str {
    "<i>Upgrade.</i>
    Regular promotion rules. Note that there is currently no UI to choose promotion
    target. By default a pawn will be promoted to a Queen. Hold Shift to promote to a
    Knight. Use algebraic input to promote to a Rook or a Bishop."
}
pub fn promotion_discard_tooltip() -> &'static str {
    "<i>Discard.</i>
    Upon reaching the last rank the pawn is lost and goes to your opponent's reserve. You
    get nothing. C'est la vie."
}
pub fn promotion_steal_tooltip() -> &'static str {
    "<i>Steal.</i>
    Expropriate your partner opponent's piece when promoting a pawn! Can only steal a
    piece from the board, not from reserve. Cannot check player by stealing their piece."
}

pub fn drop_no_check_aggression_tooltip() -> &'static str {
    "<i>No check.</i>
    Drop with a check is forbidden."
}
pub fn drop_no_chess_mate_aggression_tooltip() -> &'static str {
    "<i>No chess mate.</i>
    Drop with a checkmate is forbidden, even if the opponent can escape the checkmate with
    a drop of their own."
}
pub fn drop_no_bughouse_mate_aggression_tooltip() -> &'static str {
    "<i>No bughouse mate.</i>
    Drop with a checkmate is forbidden, unless the opponent can escape the checkmate with
    a drop of their own (even if their reserve is currently empty)."
}
pub fn drop_mate_allowed_aggression_tooltip() -> &'static str {
    "<i>Mate allowed.</i>
    Drop with a checkmate is allowed."
}

pub fn pawn_drop_rank_tooltip() -> &'static [&'static str] {
    &[
        "Allowed pawn drop ranks in “min-max” format. Ranks are counted starting from the player,
        so “2-6” means White can drop from rank 2 to rank 6 and Black can drop
        from rank 7 to rank 3.",
        "Limitations:<br> 1 ≤ min ≤ max ≤ 7",
    ]
}

pub fn make_new_match_rules_body() -> String {
    [
        RuleNode::new(FAIRY_PIECES, "Fairy pieces")
            .with_input_select([("off", "—", true), ("accolade", "Accolade", false)])
            .with_tooltip([accolade_tooltip()]),
        RuleNode::new(STARTING_POSITION, "Starting position")
            .with_input_select([
                ("off", "—", false),
                ("fischer-random", "Fischer random", true),
            ])
            .with_tooltip([fischer_random_tooltip()]),
        RuleNode::new(DUCK_CHESS, "Duck chess")
            .with_input_select([("off", "—", true), ("on", "Duck chess", false)])
            .with_tooltip([duck_chess_tooltip()]),
        RuleNode::new(FOG_OF_WAR, "Fog of war")
            .with_input_select([("off", "—", true), ("on", "Fog of war", false)])
            .with_tooltip([fog_of_war_tooltip()]),
        RuleNode::new(STARTING_TIME, "Starting time")
            .with_input_text("[0-9]+:[0-5][0-9]", "m:ss", "5:00")
            .with_tooltip([stating_time_tooltip()]),
        RuleNode::new(PROMOTION, "Promotion")
            .with_input_select([
                ("upgrade", "Upgrade", true),
                ("discard", "Discard", false),
                ("steal", "Steal", false),
            ])
            .with_tooltip([
                promotion_upgrade_tooltip(),
                promotion_discard_tooltip(),
                promotion_steal_tooltip(),
            ]),
        RuleNode::new(DROP_AGGRESSION, "Drop aggression")
            .with_input_select([
                ("no-check", "No check", false),
                ("no-chess-mate", "No chess mate", true),
                ("no-bughouse-mate", "No bughouse mate", false),
                ("mate-allowed", "Mate allowed", false),
            ])
            .with_tooltip([
                drop_no_check_aggression_tooltip(),
                drop_no_chess_mate_aggression_tooltip(),
                drop_no_bughouse_mate_aggression_tooltip(),
                drop_mate_allowed_aggression_tooltip(),
            ]),
        RuleNode::new(PAWN_DROP_RANKS, "Pawn drop ranks")
            .with_input_text(
                "1-[1-7]|2-[2-7]|3-[3-7]|4-[4-7]|5-[5-7]|6-[6-7]|7-[7-7]",
                "min-max",
                "2-6",
            )
            .with_tooltip(pawn_drop_rank_tooltip()),
    ]
    .map(|rule| rule.to_html())
    .join("")
}
