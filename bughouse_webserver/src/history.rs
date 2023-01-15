use crate::{GroupStats, RawStats};

// for colorizer plugins in editors
const fn rgb(x: u8, y: u8, z: u8) -> (u8, u8, u8) {
    (x, y, z)
}

// https://google.github.io/palette.js
const COLOR_PALETTE: [(u8, u8, u8); 8] = [
    rgb(255, 0, 41),
    rgb(55, 126, 184),
    rgb(102, 166, 30),
    rgb(152, 78, 163),
    rgb(0, 210, 213),
    rgb(255, 127, 0),
    rgb(175, 141, 0),
    rgb(127, 128, 205),
];

const DASH_TYPE_PALETTE: [plotly::common::DashType; 6] = [
    plotly::common::DashType::Solid,
    plotly::common::DashType::Dot,
    plotly::common::DashType::Dash,
    plotly::common::DashType::LongDash,
    plotly::common::DashType::DashDot,
    plotly::common::DashType::LongDashDot,
];

struct Style {
    line_color: plotly::color::Rgba,
    line_dash_type: plotly::common::DashType,
    fill_color: plotly::color::Rgba,
}

fn style_for_index(index: usize) -> Style {
    let i = index % COLOR_PALETTE.len();
    let j = (index / COLOR_PALETTE.len()) % DASH_TYPE_PALETTE.len();
    let (r, g, b) = COLOR_PALETTE[i];
    Style {
        line_color: plotly::color::Rgba::new(r, g, b, 1.0),
        line_dash_type: DASH_TYPE_PALETTE[j].clone(),
        fill_color: plotly::color::Rgba::new(r, g, b, 0.1),
    }
}

// Formats the update timestamp in a way that is accepted by Plotly.
fn get_timestamp_for_plotly(stats: &RawStats) -> Option<String> {
    stats
        .last_update?
        .format(time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second]"
        ))
        .ok()
}

#[derive(Clone, Copy)]
pub enum XAxis {
    Timestamp,
    UpdateIndex,
}

pub fn players_rating_graph_html(stats: &GroupStats<Vec<RawStats>>, x_axis: XAxis) -> String {
    let mut plot = plotly::Plot::new();
    let layout = plot.layout().clone().title("Player rating history".into());
    plot.set_layout(layout);
    for (index, (player, stats_vec)) in stats.per_player.iter().enumerate() {
        // Drops points where the timestamp or rating can't be determined.
        let filtered_stats = stats_vec
            .iter()
            .filter(|stat| stat.last_update.is_some() && stat.rating.is_some());

        let xs = make_xs(filtered_stats.clone(), x_axis);

        // filter_map is unnecessary here and below, but avoids unwraps.
        let rating = filtered_stats
            .clone()
            .filter_map(|stat| stat.rating.map(|r| r.rating))
            .collect::<Vec<_>>();
        let lower_rating = filtered_stats
            .clone()
            .filter_map(|stat| stat.rating.map(|r| r.rating - r.uncertainty))
            .collect::<Vec<_>>();
        let upper_rating = filtered_stats
            .clone()
            .filter_map(|stat| stat.rating.map(|r| r.rating + r.uncertainty))
            .collect::<Vec<_>>();

        let Style {
            line_color,
            line_dash_type,
            fill_color,
        } = style_for_index(index);

        let lower_rating_trace = plotly::Scatter::new(xs.clone(), lower_rating)
            .mode(plotly::common::Mode::Lines)
            .fill(plotly::common::Fill::None)
            // Use fill color so that the line is not visible
            .line(plotly::common::Line::default().color(fill_color).width(1.))
            .show_legend(false)
            .legend_group(player)
            .hover_info(plotly::common::HoverInfo::Skip);
        let upper_rating_trace = plotly::Scatter::new(xs.clone(), upper_rating)
            .mode(plotly::common::Mode::Lines)
            .fill(plotly::common::Fill::ToNextY)
            .fill_color(fill_color)
            // Use fill color so that the line is not visible
            .line(plotly::common::Line::default().color(fill_color).width(1.))
            .show_legend(false)
            .legend_group(player)
            .hover_info(plotly::common::HoverInfo::Skip);
        let rating_trace = plotly::Scatter::new(xs, rating)
            .name(player)
            .mode(plotly::common::Mode::LinesMarkers)
            .line(
                plotly::common::Line::default()
                    .color(line_color)
                    .dash(line_dash_type),
            )
            .marker(plotly::common::Marker::default().size(4))
            .legend_group(player);

        plot.add_trace(lower_rating_trace);
        plot.add_trace(upper_rating_trace);
        plot.add_trace(rating_trace);
    }
    plot.to_inline_html(None)
}

pub fn teams_elo_graph_html(stats: &GroupStats<Vec<RawStats>>, x_axis: XAxis) -> String {
    let mut plot = plotly::Plot::new();
    let layout = plot.layout().clone().title("Team Elo history".into());
    plot.set_layout(layout);
    for (index, (team, stats_vec)) in stats.per_team.iter().enumerate() {
        // Drops points where the timestamp or rating can't be determined.
        let filtered_stats = stats_vec
            .iter()
            .filter(|stat| stat.last_update.is_some() && stat.elo.is_some());

        // filter_map is unnecessary here and below, but avoids unwraps.
        let xs = make_xs(filtered_stats.clone(), x_axis);

        let elo = filtered_stats
            .clone()
            .filter_map(|stat| stat.elo.map(|r| r.rating))
            .collect::<Vec<_>>();

        let team_str = format!("{}, {}", team[0], team[1]);

        let Style {
            line_color,
            line_dash_type,
            ..
        } = style_for_index(index);

        let elo_trace = plotly::Scatter::new(xs, elo)
            .name(team_str.clone())
            .mode(plotly::common::Mode::LinesMarkers)
            .line(
                plotly::common::Line::default()
                    .color(line_color)
                    .dash(line_dash_type),
            )
            .marker(plotly::common::Marker::default().size(4))
            .legend_group(team_str);

        plot.add_trace(elo_trace);
    }
    plot.to_inline_html(None)
}

fn make_xs<'a, I: Iterator<Item = &'a RawStats>>(stats: I, x_axis: XAxis) -> Vec<String> {
    match x_axis {
        XAxis::Timestamp => stats
            .filter_map(get_timestamp_for_plotly)
            .collect::<Vec<_>>(),
        XAxis::UpdateIndex => stats
            .map(|s| format!("{}", s.update_index))
            .collect::<Vec<_>>(),
    }
}
