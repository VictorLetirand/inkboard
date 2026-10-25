//! Samples and their summary (median, p95, p99...), and a few formatting helpers for the reports.

use serde::{Deserialize, Serialize};

/// A bag of measurements (any unit): summarized once at the end.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Samples(pub Vec<f64>);

impl Samples {
    pub fn add(&mut self, v: f64) {
        self.0.push(v);
    }
    pub fn merge(&mut self, other: &Samples) {
        self.0.extend_from_slice(&other.0);
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    #[must_use]
    pub fn summary(&self) -> Summary {
        Summary::of(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { 0.0 } else { sorted[((sorted.len() - 1) as f64 * p).round() as usize] }
}

impl Summary {
    pub fn of(values: &[f64]) -> Self {
        let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
        v.sort_by(f64::total_cmp);
        let n = v.len();
        Self {
            n,
            min: v.first().copied().unwrap_or(0.0),
            p50: pct(&v, 0.5),
            p95: pct(&v, 0.95),
            p99: pct(&v, 0.99),
            max: v.last().copied().unwrap_or(0.0),
            mean: if n == 0 { 0.0 } else { v.iter().sum::<f64>() / n as f64 },
        }
    }

    /// `p50 / p95 / p99` with one decimal, or `-` when there is no sample.
    #[must_use]
    pub fn triple(&self) -> String {
        if self.n == 0 { "-".to_owned() } else { format!("{:.1} / {:.1} / {:.1}", self.p50, self.p95, self.p99) }
    }
}

/// Mebibytes with one decimal.
#[must_use]
pub fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_of_a_known_series() {
        let s = Summary::of(&(1..=100).map(f64::from).collect::<Vec<_>>());
        assert_eq!((s.n, s.min, s.max), (100, 1.0, 100.0));
        assert_eq!((s.p50, s.p95, s.p99), (51.0, 95.0, 99.0));
        assert_eq!(Summary::of(&[]).triple(), "-");
    }
}
