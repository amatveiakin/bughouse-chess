use std::{fmt, iter};

use itertools::Itertools;


pub const PLAYER_NAME: &'static str = "player_name"; // filled by JSs
pub const RATING: &'static str = "rating"; // filled by JSs
pub const FAIRY_PIECES: &'static str = "fairy_pieces";
pub const STARTING_POSITION: &'static str = "starting_position";
pub const DUCK_CHESS: &'static str = "duck_chess";
pub const FOG_OF_WAR: &'static str = "fog_of_war";
pub const STARTING_TIME: &'static str = "starting_time";
pub const PROMOTION: &'static str = "promotion";
pub const PAWN_DROP_RANKS: &'static str = "pawn_drop_ranks";
pub const DROP_AGGRESSION: &'static str = "drop_aggression";

pub const REGICIDE_CLASS: &'static str = "rule-warning-regicide";
const REGICIDE_ICON: &'static str = "
    <svg class='inline-icon' viewBox='0 0 100 100'>
        <path d='m36.408 23.945 6.7005-1.0273 1.0304 5.557 6.5262-1.0006 1.0273 6.7005-6.3164 0.96845 1.4693 7.5385q0.15051-0.24388 0.31416-0.48112c5.1065-7.4193 13.621-12.256 21.157-7.7163 7.5558 4.5565 9.5247 17.398 1.4384 26.24-4.5808 5.0085-8.64 9.782-7.4995 15.679l9.5854-0.1708 1.2842 8.3756-46.865 7.1854-1.2842-8.3756 9.1963-2.7089c-0.67859-5.9677-5.9815-9.3058-11.852-12.712-10.36-6.0133-12.329-18.855-6.4892-25.466 5.8245-6.5881 15.398-4.5257 22.497 1.0231q0.22734 0.17816 0.44388 0.3649l-0.85676-7.6324-6.3249 0.96974-1.0273-6.7005 6.5262-1.0006zm-2.3779 26.622c5.123 6.0786 6.8673 13.841 7.3894 17.246-2.0524 0.31469-10.311-1.8992-15.264-6.8591-3.5652-3.5708-5.2493-9.4216-3.0767-12.126s7.7492-2.0601 10.951 1.7396zm19.323-2.9627c-3.0665 7.3342-2.4051 15.262-1.883 18.668 2.0524-0.31469 9.268-4.9011 12.508-11.117 2.3316-4.4749 2.1855-10.561-0.6977-12.491-2.8832-1.9297-8.0105 0.35621-9.927 4.9406z' fill='#808080' fill-rule='evenodd' stroke='#000' stroke-width='.5'/>
        <path d='m84.231 3.0957c-0.16924 0.01262-0.32906 0.10655-0.41992 0.26562l-2.0918 3.6602c-0.14538 0.25452-0.05726 0.57532 0.19727 0.7207l0.71289 0.4082-4.957 8.6777-9.3477-5.3398c-0.51521-0.29428-1.1666-0.11482-1.4609 0.40039l-2.6113 4.5684c-0.29429 0.51521-0.11482 1.1667 0.40039 1.4609l8.2266 4.6992-27.125 47.488 0.09961 8.4238 7.3047-4.1934 27.125-47.488 8.2285 4.6992c0.51521 0.29428 1.1666 0.11677 1.4609-0.39844l2.6094-4.5703c0.29429-0.51521 0.11678-1.1667-0.39843-1.4609l-9.3477-5.3398 4.957-8.6777 0.71289 0.40625c0.25453 0.14538 0.57728 0.05726 0.72266-0.19726l2.0898-3.6582c0.14539-0.25452 0.05726-0.57727-0.19726-0.72266l-6.5898-3.7637c-0.09544-0.05452-0.19924-0.07593-0.30078-0.068359z' stroke='#fff' stroke-linejoin='round'/>
    </svg>
";

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

    pub fn input_id(&self) -> String { format!("rule-input-{}", self.name) }
    pub fn class(&self) -> String { rule_setting_class(&self.name) }

    pub fn with_input_select<S1: fmt::Display, S2: fmt::Display>(
        mut self, options: impl IntoIterator<Item = (S1, S2, bool)>,
    ) -> Self {
        let id = self.input_id();
        let name = &self.name;
        let class = self.class();
        let mut num_selected = 0;
        self.input = Some(format!(
            "<select id={id} name='{name}' class='{class}'>{}</select>",
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
        let id = self.input_id();
        let name = &self.name;
        let class = self.class();
        self.input = Some(format!(
            "<input type='text' id={id} name='{name}' class='{class}'
            pattern='{pattern}' placeholder='{placeholder}' value='{value}'
            spellcheck='false' autocomplete='off' required/>"
        ));
        self
    }

    pub fn with_tooltip<S: fmt::Display>(
        mut self, paragraphs: impl IntoIterator<Item = S>,
    ) -> Self {
        self.tooltip =
            Some(standalone_tooltip(&paragraphs_to_html(paragraphs), [self.class().as_str()]));
        self
    }

    pub fn to_html(&self) -> String {
        let mut html = String::new();
        let id = self.input_id();
        let class = self.class();
        let label_text = html_escape::encode_text(&self.label);
        html.push_str(&format!("<label for='{id}' class='{class}'>{label_text}</label>"));
        html.push_str(self.input.as_ref().unwrap());
        if let Some(tooltip) = &self.tooltip {
            html.push_str(&tooltip);
        } else {
            html.push_str(&format!("<div class='{class}'></div>"))
        }
        html
    }
}

pub fn rule_setting_class(name: &str) -> String { format!("rule-setting-{}", name) }

fn paragraphs_to_html<S: fmt::Display>(paragraphs: impl IntoIterator<Item = S>) -> String {
    paragraphs.into_iter().map(|p| format!("<p>{}</p>", p)).join("")
}

fn standalone_tooltip<'a>(
    text: &str, additional_classes: impl IntoIterator<Item = &'a str>,
) -> String {
    format!(
        "<div class='{}'><div class='tooltip-text'>{text}</div></div>",
        iter::once("tooltip-standalone").chain(additional_classes).join(" ")
    )
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

pub fn regicide_tooltip() -> &'static [&'static str] {
    &[
        "<i>Regicide.</i>
        Capture opponent's king in order to win the game. There are no checks and checkmates.
        Drop aggression is always “Mate Allowed”. Attacking the king does not prevent castling.",
        "This option is implied by certain game variants, namely Duck chess and Fog of war.",
    ]
}

pub fn make_new_match_rules_body() -> String {
    let mut rows = [
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
        RuleNode::new(PAWN_DROP_RANKS, "Pawn drop ranks")
            .with_input_text(
                "1-[1-7]|2-[2-7]|3-[3-7]|4-[4-7]|5-[5-7]|6-[6-7]|7-[7-7]",
                "min-max",
                "2-6",
            )
            .with_tooltip(pawn_drop_rank_tooltip()),
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
    ]
    .into_iter()
    .map(|rule| rule.to_html())
    .collect_vec();

    rows.push(format!(
        "<div class='grid-col-span-2 flex-row gap-medium {REGICIDE_CLASS}'>
            {REGICIDE_ICON}
            <div class='inline-block'>
                Regicide: no checks and mates
            </div>
        </div>
        {}",
        standalone_tooltip(&paragraphs_to_html(regicide_tooltip()), [REGICIDE_CLASS])
    ));

    rows.join("")
}
