//! Doubles of this crate's ports for other crates' tests.

use crate::{MetricsSink, Reading};
use std::sync::Mutex;

/// Keeps every batch it is given, or refuses them all when made with `failing`.
#[derive(Default)]
pub struct InMemoryMetricsSink {
    batches: Mutex<Vec<Vec<Reading>>>,
    attempts: Mutex<usize>,
    failing: bool,
}

impl InMemoryMetricsSink {
    pub fn failing() -> Self {
        Self { failing: true, ..Self::default() }
    }

    /// The batches accepted, in order.
    pub fn batches(&self) -> Vec<Vec<Reading>> {
        self.batches.lock().unwrap().clone()
    }

    /// Calls made, accepted or not: proves a request arrived at a sink that refused it.
    pub fn attempts(&self) -> usize {
        *self.attempts.lock().unwrap()
    }
}

impl MetricsSink for InMemoryMetricsSink {
    fn record(&self, readings: &[Reading]) -> Result<(), String> {
        *self.attempts.lock().unwrap() += 1;
        if self.failing {
            return Err("sink refused the batch".to_string());
        }
        self.batches.lock().unwrap().push(readings.to_vec());
        Ok(())
    }
}
