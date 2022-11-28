// Improvement potential. Synchronized version for server-side logging.

use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use hdrhistogram::Histogram;
use serde::{Serialize, Deserialize};


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeterStats {
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub num_values: u64,
}

impl fmt::Display for MeterStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.num_values;
        if n == 0 {
            write!(f, "- (N={n})")
        } else {
            write!(f, "P50={}, P90={}, P99={} (N={n})", self.p50, self.p90, self.p99)
        }
    }
}


pub struct MeterBox {
    meters: HashMap<String, Meter>,
}

impl MeterBox {
    pub fn new() -> Self {
        MeterBox {
            meters: HashMap::new(),
        }
    }

    pub fn meter(&mut self, name: String) -> Meter {
        self.meters.entry(name).or_insert_with(|| Meter::new()).clone()
    }

    pub fn read_stats(&self) -> HashMap<String, MeterStats> {
        self.meters.iter()
            .map(|(name, meter)| (name.clone(), meter.stats()))
            .collect()
    }
    pub fn consume_stats(&mut self) -> HashMap<String, MeterStats> {
        let stats = self.read_stats();
        self.meters.clear();
        stats
    }
}


#[derive(Clone)]
pub struct Meter {
    histogram: Rc<RefCell<Histogram<u64>>>,
}

impl Meter {
    fn new() -> Self {
        const SIGNIFICANT_DIGITS: u8 = 3;
        Meter {
            histogram: Rc::new(RefCell::new(Histogram::new(SIGNIFICANT_DIGITS).unwrap())),
        }
    }

    pub fn record(&self, value: u64) {
        self.histogram.borrow_mut().record(value).unwrap()
    }
    pub fn record_duration(&self, duration: Duration) {
        let value = cmp::min(duration.as_millis(), u64::MAX.into()).try_into().unwrap();
        self.record(value);
    }

    fn stats(&self) -> MeterStats {
        let histogram = self.histogram.borrow();
        MeterStats {
            p50: histogram.value_at_quantile(0.5),
            p90: histogram.value_at_quantile(0.9),
            p99: histogram.value_at_quantile(0.99),
            num_values: histogram.len()
        }
    }
}
