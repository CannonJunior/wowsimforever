//! Shared Monte Carlo output statistics (Plan Section 3, Principle 9).
//!
//! Factored out of `wowforever-engine` so both the steady-state DPS engine
//! and the burst calculator (`wowforever-burst`) report sampled
//! distributions the same way, rather than maintaining two independent
//! implementations of "turn a list of samples into mean/stdev/percentiles"
//! that could silently drift apart.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpsStats {
    pub mean: f64,
    pub stdev: f64,
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
}

pub fn stats_from(samples: &mut [f64]) -> DpsStats {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = samples.len();
    let mean = samples.iter().sum::<f64>() / n as f64;
    let var = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    let pct = |p: f64| samples[((p * (n as f64 - 1.0)).round() as usize).min(n - 1)];
    DpsStats { mean, stdev: var.sqrt(), p5: pct(0.05), p50: pct(0.50), p95: pct(0.95) }
}
