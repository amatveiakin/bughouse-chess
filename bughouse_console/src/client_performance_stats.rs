use bughouse_chess::meter::MeterStats;
use hdrhistogram::Histogram;
use itertools::Itertools;


#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ClientPerformanceRecord {
    pub git_version: String,
    pub user_agent: String,
    pub time_zone: String,
    pub ping_stats: MeterStats,
    pub update_state_stats: MeterStats,
    pub turn_confirmation_stats: MeterStats,
}

// TODO: What is the proper way to aggregate quantiles?
#[derive(Clone, Debug)]
pub struct MeterStatsWithUncertainty {
    pub p50s: Histogram<u64>,
    pub p90s: Histogram<u64>,
    pub p99s: Histogram<u64>,
}

pub struct AggregatedClientPerformancePoint {
    pub git_version: String,
    pub ping_stats: MeterStatsWithUncertainty,
    pub update_state_stats: MeterStatsWithUncertainty,
    pub turn_confirmation_stats: MeterStatsWithUncertainty,
}

// TODO: Add performance stats by browsers and ping by location.
pub struct ClientPerformanceStats {
    points: Vec<AggregatedClientPerformancePoint>,
}

impl MeterStatsWithUncertainty {
    pub fn from_values(values: &[MeterStats]) -> Self {
        let p50s = make_histogram(values.iter().map(|stats| stats.p50));
        let p90s = make_histogram(values.iter().map(|stats| stats.p90));
        let p99s = make_histogram(values.iter().map(|stats| stats.p99));
        Self { p50s, p90s, p99s }
    }
}

impl ClientPerformanceStats {
    // Improvement potential. Consider removing git versions with too few data points.
    pub fn from_values(records: Vec<ClientPerformanceRecord>) -> Self {
        let points = records
            .into_iter()
            .chunk_by(|record| record.git_version.clone())
            .into_iter()
            .map(|(git_version, records)| {
                let mut ping_values = vec![];
                let mut update_state_values = vec![];
                let mut turn_confirmation_values = vec![];
                for record in records {
                    ping_values.push(record.ping_stats);
                    update_state_values.push(record.update_state_stats);
                    turn_confirmation_values.push(record.turn_confirmation_stats);
                }
                let ping_stats = MeterStatsWithUncertainty::from_values(&ping_values);
                let update_state_stats =
                    MeterStatsWithUncertainty::from_values(&update_state_values);
                let turn_confirmation_stats =
                    MeterStatsWithUncertainty::from_values(&turn_confirmation_values);
                AggregatedClientPerformancePoint {
                    git_version,
                    ping_stats,
                    update_state_stats,
                    turn_confirmation_stats,
                }
            })
            .collect();
        Self { points }
    }
}

fn make_histogram(values: impl IntoIterator<Item = u64>) -> Histogram<u64> {
    const SIGNIFICANT_DIGITS: u8 = 3;
    let mut histogram = Histogram::<u64>::new(SIGNIFICANT_DIGITS).unwrap();
    for value in values {
        histogram.record(value).unwrap();
    }
    histogram
}

fn make_trace(
    stats: &ClientPerformanceStats, xs: &[String], name: &str,
    get_y: impl Fn(&AggregatedClientPerformancePoint) -> &Histogram<u64>,
) -> Box<plotly::Scatter<String, u64>> {
    // TODO: Add other quantiles to see things like “how fast the game is on average for 10% slowest
    // devices” and “what is the ~slowest case (99th percentile of the 99th percentile)”.
    let ys = stats.points.iter().map(|p| get_y(p).value_at_quantile(0.5)).collect_vec();
    plotly::Scatter::new(xs.to_vec(), ys)
        .name(name)
        .mode(plotly::common::Mode::LinesMarkers)
        .marker(plotly::common::Marker::default().size(4))
}

pub fn performance_stats_graph_html(stats: &ClientPerformanceStats) -> String {
    let mut plot = plotly::Plot::new();
    let layout = plot
        .layout()
        .clone()
        .title("Client performance: mean time per operation (milliseconds)");
    plot.set_layout(layout);
    let xs = stats.points.iter().map(|p| p.git_version.clone()).collect_vec();
    plot.add_trace(make_trace(stats, &xs, "ping_50", |p| &p.ping_stats.p50s));
    plot.add_trace(make_trace(stats, &xs, "ping_90", |p| &p.ping_stats.p90s));
    plot.add_trace(make_trace(stats, &xs, "ping_99", |p| &p.ping_stats.p99s));
    plot.add_trace(make_trace(stats, &xs, "update_state_50", |p| &p.update_state_stats.p50s));
    plot.add_trace(make_trace(stats, &xs, "update_state_90", |p| &p.update_state_stats.p90s));
    plot.add_trace(make_trace(stats, &xs, "update_state_99", |p| &p.update_state_stats.p99s));
    plot.add_trace(make_trace(stats, &xs, "turn_confirmation_50", |p| {
        &p.turn_confirmation_stats.p50s
    }));
    plot.add_trace(make_trace(stats, &xs, "turn_confirmation_90", |p| {
        &p.turn_confirmation_stats.p90s
    }));
    plot.add_trace(make_trace(stats, &xs, "turn_confirmation_99", |p| {
        &p.turn_confirmation_stats.p99s
    }));
    plot.to_inline_html(None)
}
