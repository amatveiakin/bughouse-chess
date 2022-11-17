// Improvement potential. Synchronized version for server-side logging.

use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use hdrhistogram::Histogram;
use itertools::Itertools;


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

    pub fn statistics(&self) -> String {
        self.meters.iter()
            .sorted_by_key(|(name, _)| name.as_str())
            .map(|(name, meter)| format!("{}: {}", name, meter.statistics()))
            .join("\n")
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

    fn statistics(&self) -> String {
        let histogram = self.histogram.borrow();
        let n = histogram.len();
        if n == 0 {
            format!("- (N={n})")
        } else {
            let p50 = histogram.value_at_quantile(0.5);
            let p90 = histogram.value_at_quantile(0.9);
            let p99 = histogram.value_at_quantile(0.99);
            format!("P50={p50}, P90={p90}, P99={p99} (N={n})")
        }
    }
}
