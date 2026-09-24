//! Machine readings for the bottom-right corner and for the `/machine`
//! panel: CPU, RAM and GPU memory as percentages (and swap, if any),
//! sampled on a dedicated thread every 5 s when idle and twice a second
//! while the model thinks or replies or the panel is open, plus the peak
//! over the last 3 minutes. It matters because the models run locally: if
//! RAM runs out or the model falls into swap, generation crawls.
//!
//! The model's own footprint, as the provider reports it, goes into each
//! sample too: how much of it sits in RAM is drawn under the ram curve, and
//! on Linux it is added to the used total, because there the kernel counts
//! a model mapped from disk as cache and leaves it out.

pub mod gpu;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use moon_core::LoadedModel;
use sysinfo::System;

use crate::app::{Action, LoadedState, Tx};
use gpu::Gpu;

/// How often a sample is taken when idle.
pub const IDLE_INTERVAL: Duration = Duration::from_secs(5);
/// How often while the model thinks or replies, or while the panel that
/// draws the machine is open. Above sysinfo's minimum interval for a
/// trustworthy cpu reading (200 ms).
pub const BUSY_INTERVAL: Duration = Duration::from_millis(500);
/// Window over which the peak is computed.
pub const WINDOW: Duration = Duration::from_secs(180);

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Linux counts a file mapped into memory as cache, reclaimable, even while
/// the model reads it on every token: what the system reports as used
/// leaves the model out. There, the model's share in RAM goes on top of it.
/// macOS wires those pages and counts them; Windows is left as it reports.
pub const MODEL_COUNTS_AS_CACHE: bool = cfg!(target_os = "linux");

/// On a Mac the GPU shares the RAM: the model is in RAM whether Ollama
/// calls it VRAM or not.
const UNIFIED_MEMORY: bool = cfg!(target_os = "macos");

/// One reading of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sample {
    /// CPU usage, average over all cores, 0–100.
    pub cpu: f32,
    /// RAM used over the total, 0–100.
    pub ram: f32,
    /// What the system reports as used, plus the model where the system
    /// leaves it out (`MODEL_COUNTS_AS_CACHE`).
    pub ram_used: u64,
    pub ram_total: u64,
    pub swap_used: u64,
    /// Bytes of the loaded model that sit in RAM: drawn under the ram curve.
    pub model_ram: u64,
    /// GPU memory used over the total, 0–100; zero without a card to read.
    pub gpu: f32,
    pub gpu_used: u64,
    pub gpu_total: u64,
}

impl Sample {
    pub fn new(cpu: f32, ram_used: u64, ram_total: u64, swap_used: u64) -> Self {
        Self {
            cpu,
            ram: pct(ram_used, ram_total),
            ram_used,
            ram_total,
            swap_used,
            ..Self::default()
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

    /// Notes the bytes of the model that live in RAM and, where the system
    /// leaves them out of its used total (`stack`), adds them to it.
    pub fn with_model(mut self, bytes: u64, stack: bool) -> Self {
        self.model_ram = bytes;
        if stack && bytes > 0 {
            self.ram_used = self.ram_used.saturating_add(bytes).min(self.ram_total);
            self.ram = pct(self.ram_used, self.ram_total);
        }
        self
    }

    pub fn with_gpu(mut self, used: u64, total: u64) -> Self {
        self.gpu_used = used;
        self.gpu_total = total;
        self.gpu = pct(used, total);
        self
    }
}

fn pct(part: u64, total: u64) -> f32 {
    if total > 0 {
        (part as f64 / total as f64 * 100.0) as f32
    } else {
        0.0
    }
}

/// Bytes of the loaded model that sit in RAM: on a Mac the whole of it, the
/// GPU sharing the memory; elsewhere, what did not fit in the GPU.
pub fn model_in_ram(loaded: &LoadedState) -> u64 {
    match loaded {
        LoadedState::Loaded(m) => model_in_ram_of(m, UNIFIED_MEMORY),
        _ => 0,
    }
}

fn model_in_ram_of(m: &LoadedModel, unified: bool) -> u64 {
    if unified {
        m.size_bytes
    } else {
        m.size_bytes.saturating_sub(m.size_vram_bytes)
    }
}

/// Sampling pace, shared with the thread: the interface sets it to fast while
/// there is something worth watching closely and back to slow when there is
/// not.
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

    pub fn peak_gpu(&self) -> f32 {
        self.samples.iter().map(|(_, s)| s.gpu).fold(0.0, f32::max)
    }

    /// Points for a chart, oldest first: on `x`, seconds from now (negative,
    /// down to `-WINDOW`); on `y`, what `value` reads from the sample.
    pub fn series_at(&self, now: Instant, value: impl Fn(&Sample) -> f32) -> Vec<(f64, f64)> {
        self.samples
            .iter()
            .map(|(t, s)| (-now.duration_since(*t).as_secs_f64(), value(s) as f64))
            .collect()
    }

    pub fn series(&self, value: impl Fn(&Sample) -> f32) -> Vec<(f64, f64)> {
        self.series_at(Instant::now(), value)
    }

    /// Span the samples cover, in seconds.
    pub fn span(&self) -> f64 {
        match (self.samples.front(), self.samples.back()) {
            (Some((a, _)), Some((b, _))) => b.duration_since(*a).as_secs_f64(),
            _ => 0.0,
        }
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
/// the difference between two readings. The GPU, if there is one to read, is
/// found once, on the thread, since loading NVML can take a moment. The
/// thread wakes at the fast pace to notice a `pace` change right away, but
/// only measures when due according to the current pace. It ends on its own
/// when the receiver goes away.
pub fn spawn(tx: Tx, pace: Pace) {
    let spawned = std::thread::Builder::new()
        .name("sysmon".into())
        .spawn(move || {
            let mut sys = System::new();
            let gpu = Gpu::detect();
            // the first CPU reading comes out as zero: it only serves as a baseline
            sys.refresh_cpu_usage();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            let mut last: Option<Instant> = None;
            loop {
                if last.is_none_or(|t| t.elapsed() >= pace.interval()) {
                    sys.refresh_cpu_usage();
                    sys.refresh_memory();
                    let mut sample = Sample::from_system(&sys);
                    if let Some((used, total)) = gpu.as_ref().and_then(Gpu::read) {
                        sample = sample.with_gpu(used, total);
                    }
                    if tx.send(Action::SysSample(sample)).is_err() {
                        break;
                    }
                    last = Some(Instant::now());
                }
                // the shortest pace there is: any longer and a change of pace
                // would take that long to be noticed
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
        assert_eq!(
            (x.gpu, x.gpu_used, x.gpu_total, x.model_ram),
            (0.0, 0, 0, 0)
        );
    }

    #[test]
    fn the_model_goes_under_the_curve_and_on_linux_on_top_of_the_total() {
        let x = Sample::new(10.0, 8 << 30, 32 << 30, 0);
        // where the system already counts it: noted, nothing added
        let y = x.with_model(4 << 30, false);
        assert_eq!((y.model_ram, y.ram_used, y.ram), (4 << 30, 8 << 30, 25.0));
        // where it goes as cache: added to what is used, never past the total
        let z = x.with_model(4 << 30, true);
        assert_eq!((z.model_ram, z.ram_used, z.ram), (4 << 30, 12 << 30, 37.5));
        let over = x.with_model(30 << 30, true);
        assert_eq!((over.ram_used, over.ram), (32 << 30, 100.0));
        // no model: nothing changes either way
        assert_eq!(x.with_model(0, true), x);
    }

    #[test]
    fn what_of_the_model_is_in_ram_depends_on_the_memory_being_shared() {
        let m = LoadedModel {
            id: "m".into(),
            size_bytes: 10 << 30,
            size_vram_bytes: 7 << 30,
            context_length: None,
            expires_at: None,
        };
        // a card of its own: what spilled out of it
        assert_eq!(model_in_ram_of(&m, false), 3 << 30);
        // shared memory: all of it, whatever the provider calls vram
        assert_eq!(model_in_ram_of(&m, true), 10 << 30);
        assert_eq!(model_in_ram(&LoadedState::NotLoaded), 0);
        assert_eq!(model_in_ram(&LoadedState::Unknown), 0);
    }

    #[test]
    fn the_gpu_reading_is_a_percentage_of_its_own() {
        let x = Sample::new(10.0, 8 << 30, 32 << 30, 0).with_gpu(6 << 30, 8 << 30);
        assert_eq!((x.gpu, x.gpu_used, x.gpu_total), (75.0, 6 << 30, 8 << 30));
        let mut h = History::default();
        assert_eq!(h.peak_gpu(), 0.0);
        h.push(x);
        h.push(Sample::new(0.0, 0, 0, 0).with_gpu(1, 4));
        assert_eq!(h.peak_gpu(), 75.0);
        assert_eq!(h.current().map(|s| s.gpu), Some(25.0));
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
    fn the_series_puts_the_newest_sample_at_zero() {
        let base = Instant::now();
        let mut h = History::default();
        h.push_at(base, s(10.0, 40.0));
        h.push_at(base + Duration::from_secs(5), s(20.0, 50.0));
        h.push_at(base + Duration::from_secs(10), s(30.0, 60.0));
        let now = base + Duration::from_secs(10);
        assert_eq!(
            h.series_at(now, |s| s.cpu),
            vec![(-10.0, 10.0), (-5.0, 20.0), (0.0, 30.0)]
        );
        assert_eq!(h.series_at(now, |s| s.ram).last(), Some(&(0.0, 60.0)));
        assert_eq!(h.span(), 10.0);
        assert_eq!(History::default().span(), 0.0);
    }

    #[test]
    fn the_default_pace_is_the_slow_one() {
        let p = Pace::default();
        assert!(!p.is_fast());
        assert_eq!(p.interval(), IDLE_INTERVAL);
        p.set_fast(true);
        assert_eq!(p.clone().interval(), BUSY_INTERVAL);
        p.set_fast(false);
        assert_eq!(p.interval(), IDLE_INTERVAL);
    }

    #[test]
    fn gigabytes_with_one_decimal() {
        assert_eq!(fmt_gib(32 << 30), "32.0");
        assert_eq!(fmt_gib((18u64 << 30) + (200 << 20)), "18.2");
    }
}
