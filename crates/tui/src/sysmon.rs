//! Machine readings for the bottom-right corner: CPU and RAM as percentages
//! (and swap, if any), sampled on a dedicated thread every 5 s when idle and
//! every second while the model thinks or replies, plus the peak over the
//! last 3 minutes. It matters because the models run locally: if RAM runs
//! out or the model falls into swap, generation crawls.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sysinfo::System;

use crate::app::{Action, Tx};

/// How often a sample is taken when idle.
pub const IDLE_INTERVAL: Duration = Duration::from_secs(5);
/// How often while the model thinks or replies.
pub const BUSY_INTERVAL: Duration = Duration::from_secs(1);
/// Window over which the peak is computed.
pub const WINDOW: Duration = Duration::from_secs(180);

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// One reading of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sample {
    /// CPU usage, average over all cores, 0–100.
    pub cpu: f32,
    /// RAM used over the total, 0–100.
    pub ram: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub swap_used: u64,
}

impl Sample {
    pub fn new(cpu: f32, ram_used: u64, ram_total: u64, swap_used: u64) -> Self {
        let ram = if ram_total > 0 {
            (ram_used as f64 / ram_total as f64 * 100.0) as f32
        } else {
            0.0
        };
        Self {
            cpu,
            ram,
            ram_used,
            ram_total,
            swap_used,
        }
    }

    fn from_system(sys: &System) -> Self {
        Self::new(
            sys.global_cpu_usage(),
            sys.used_memory(),
            sys.total_memory(),
            sys.used_swap(),
        )
    }
}

/// Sampling pace, shared with the thread: the interface sets it to fast while
/// the model works and back to slow when it finishes.
#[derive(Debug, Clone, Default)]
pub struct Pace(Arc<AtomicBool>);

impl Pace {
    pub fn set_fast(&self, fast: bool) {
        self.0.store(fast, Ordering::Relaxed);
    }

    pub fn is_fast(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn interval(&self) -> Duration {
        if self.is_fast() {
            BUSY_INTERVAL
        } else {
            IDLE_INTERVAL
        }
    }
}

/// The window's samples with their instant: the current one and the peak.
#[derive(Debug, Default)]
pub struct History {
    samples: VecDeque<(Instant, Sample)>,
}

impl History {
    pub fn push(&mut self, s: Sample) {
        self.push_at(Instant::now(), s);
    }

    /// Stores a sample taken at `at` and forgets those older than the window.
    pub fn push_at(&mut self, at: Instant, s: Sample) {
        while self
            .samples
            .front()
            .is_some_and(|(t, _)| at.duration_since(*t) > WINDOW)
        {
            self.samples.pop_front();
        }
        self.samples.push_back((at, s));
    }

    pub fn current(&self) -> Option<&Sample> {
        self.samples.back().map(|(_, s)| s)
    }

    pub fn peak_cpu(&self) -> f32 {
        self.samples.iter().map(|(_, s)| s.cpu).fold(0.0, f32::max)
    }

    pub fn peak_ram(&self) -> f32 {
        self.samples.iter().map(|(_, s)| s.ram).fold(0.0, f32::max)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Bytes in gigabytes with one decimal: `18.2`.
pub fn fmt_gib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / GIB)
}

/// Starts the thread that samples the machine and sends each reading over the
/// channel. A single `System` that lives the whole time: CPU is measured as
/// the difference between two readings. The thread wakes at the fast pace to
/// notice a `pace` change right away, but only measures when due according to
/// the current pace. It ends on its own when the receiver goes away.
pub fn spawn(tx: Tx, pace: Pace) {
    let spawned = std::thread::Builder::new()
        .name("sysmon".into())
        .spawn(move || {
            let mut sys = System::new();
            // the first CPU reading comes out as zero: it only serves as a baseline
            sys.refresh_cpu_usage();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            let mut last: Option<Instant> = None;
            loop {
                if last.is_none_or(|t| t.elapsed() >= pace.interval()) {
                    sys.refresh_cpu_usage();
                    sys.refresh_memory();
                    if tx
                        .send(Action::SysSample(Sample::from_system(&sys)))
                        .is_err()
                    {
                        break;
                    }
                    last = Some(Instant::now());
                }
                std::thread::sleep(BUSY_INTERVAL);
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start the machine sampler thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(cpu: f32, ram: f32) -> Sample {
        Sample {
            cpu,
            ram,
            ..Sample::default()
        }
    }

    #[test]
    fn the_ram_percentage_comes_from_the_total() {
        let x = Sample::new(10.0, 16 << 30, 32 << 30, 0);
        assert_eq!(x.ram, 50.0);
        assert_eq!(Sample::new(0.0, 5, 0, 0).ram, 0.0);
    }

    #[test]
    fn the_peak_is_the_maximum_of_the_window() {
        let mut h = History::default();
        assert!(h.current().is_none());
        assert_eq!(h.peak_cpu(), 0.0);
        h.push(s(20.0, 50.0));
        h.push(s(80.0, 40.0));
        h.push(s(30.0, 70.0));
        assert_eq!(h.current(), Some(&s(30.0, 70.0)));
        assert_eq!(h.peak_cpu(), 80.0);
        assert_eq!(h.peak_ram(), 70.0);
    }

    #[test]
    fn the_window_forgets_anything_older_than_three_minutes() {
        let base = Instant::now();
        let mut h = History::default();
        h.push_at(base, s(99.0, 99.0));
        h.push_at(base + Duration::from_secs(100), s(10.0, 10.0));
        assert_eq!(h.peak_cpu(), 99.0);
        // at 181 s the first one drops out; the one at 100 s is still inside
        h.push_at(base + Duration::from_secs(181), s(10.0, 10.0));
        assert_eq!(h.len(), 2);
        assert_eq!(h.peak_cpu(), 10.0);
        assert_eq!(h.peak_ram(), 10.0);
    }

    #[test]
    fn the_default_pace_is_the_slow_one() {
        let p = Pace::default();
        assert!(!p.is_fast());
        assert_eq!(p.interval(), IDLE_INTERVAL);
        p.set_fast(true);
        assert_eq!(p.clone().interval(), BUSY_INTERVAL);
    }

    #[test]
    fn gigabytes_with_one_decimal() {
        assert_eq!(fmt_gib(32 << 30), "32.0");
        assert_eq!(fmt_gib((18u64 << 30) + (200 << 20)), "18.2");
    }
}
