use bughouse_chess::meter::MeterStats;
use itertools::Itertools;


#[derive(Clone, Debug)]
pub struct ClientPerformanceRecord {
    pub git_version: String,
    pub user_agent: String,
    pub time_zone: String,
    pub ping_stats: MeterStats,
    pub update_state_stats: MeterStats,
}

#[derive(Clone, Copy, Debug)]
pub struct ValueWithUncertainty {
    pub mean: f64,
    pub std_dev: f64,
}

#[derive(Clone, Debug)]
pub struct MeterStatsWithUncertainty {
    pub p50: ValueWithUncertainty,
    pub p90: ValueWithUncertainty,
    pub p99: ValueWithUncertainty,
}

pub struct AggregatedClientPerformancePoint {
    pub git_version: String,
    pub ping_stats: MeterStatsWithUncertainty,
    pub update_state_stats: MeterStatsWithUncertainty,
}

// TODO: Add performance stats by browsers and ping by location.
pub struct ClientPerformanceStats {
    points: Vec<AggregatedClientPerformancePoint>,
}

impl ValueWithUncertainty {
    pub fn from_values(values: &[f64]) -> Self {
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let std_dev = values.iter().map(|value| (value - mean).powi(2)).sum::<f64>().sqrt() / n;
        Self { mean, std_dev }
    }
}

impl MeterStatsWithUncertainty {
    pub fn from_values(values: &[MeterStats]) -> Self {
        let p50_values: Vec<f64> = values.iter().map(|stats| stats.p50 as f64).collect();
        let p90_values: Vec<f64> = values.iter().map(|stats| stats.p90 as f64).collect();
        let p99_values: Vec<f64> = values.iter().map(|stats| stats.p99 as f64).collect();
        let p50 = ValueWithUncertainty::from_values(&p50_values);
        let p90 = ValueWithUncertainty::from_values(&p90_values);
        let p99 = ValueWithUncertainty::from_values(&p99_values);
        Self { p50, p90, p99 }
    }
}

impl ClientPerformanceStats {
    // Improvement potential. Consider removing git versions with too few data points.
    pub fn from_values(records: Vec<ClientPerformanceRecord>) -> Self {
        let points = records
            .into_iter()
            .group_by(|record| record.git_version.clone())
            .into_iter()
            .map(|(git_version, records)| {
                let mut ping_values = vec![];
                let mut update_state_values = vec![];
                for record in records {
                    ping_values.push(record.ping_stats);
                    update_state_values.push(record.update_state_stats);
                }
                let ping_stats = MeterStatsWithUncertainty::from_values(&ping_values);
                let update_state_stats =
                    MeterStatsWithUncertainty::from_values(&update_state_values);
                AggregatedClientPerformancePoint {
                    git_version,
                    ping_stats,
                    update_state_stats,
                }
            })
            .collect();
        Self { points }
    }
}

fn make_trace(
    stats: &ClientPerformanceStats, xs: &[String], name: &str,
    get_y: impl Fn(&AggregatedClientPerformancePoint) -> ValueWithUncertainty,
) -> Box<plotly::Scatter<String, f64>> {
    // TODO: Take std_dev into account.
    let ys = stats.points.iter().map(|point| get_y(point).mean).collect_vec();
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
        .title("Client performance: mean time per operation (milliseconds)".into());
    plot.set_layout(layout);
    let xs = stats.points.iter().map(|point| point.git_version.clone()).collect_vec();
    plot.add_trace(make_trace(stats, &xs, "ping_50", |point| point.ping_stats.p50));
    plot.add_trace(make_trace(stats, &xs, "ping_90", |point| point.ping_stats.p90));
    plot.add_trace(make_trace(stats, &xs, "ping_99", |point| point.ping_stats.p99));
    plot.add_trace(make_trace(stats, &xs, "update_state_50", |point| point.update_state_stats.p50));
    plot.add_trace(make_trace(stats, &xs, "update_state_90", |point| point.update_state_stats.p90));
    plot.add_trace(make_trace(stats, &xs, "update_state_99", |point| point.update_state_stats.p99));
    plot.to_inline_html(None)
}
