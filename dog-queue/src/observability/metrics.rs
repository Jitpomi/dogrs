use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Per-type atomic counters (updated synchronously — no locks, no spawns)
// ---------------------------------------------------------------------------

struct PerTypeCounters {
    enqueued: AtomicU64,
    completed: AtomicU64,
    failed: AtomicU64,
    retried: AtomicU64,
    canceled: AtomicU64,
}

impl PerTypeCounters {
    fn new() -> Self {
        Self {
            enqueued: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            retried: AtomicU64::new(0),
            canceled: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> JobTypeMetrics {
        JobTypeMetrics {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            retried: self.retried.load(Ordering::Relaxed),
            canceled: self.canceled.load(Ordering::Relaxed),
        }
    }
}

impl Default for PerTypeCounters {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// LiveMetrics — primary metrics store
// ---------------------------------------------------------------------------

/// Live metrics collector for queue operations.
///
/// Global counters use `AtomicU64` for wait-free reads/writes.
/// Per-job-type counters use a `DashMap` of per-entry `AtomicU64`s so that
/// `increment_*` methods are completely synchronous — no `tokio::spawn`,
/// no lock contention, no ordering surprises between global and per-type reads.
pub struct LiveMetrics {
    jobs_enqueued: AtomicU64,
    jobs_completed: AtomicU64,
    jobs_failed: AtomicU64,
    jobs_retried: AtomicU64,
    jobs_canceled: AtomicU64,

    /// Per-job-type counters. DashMap gives lock-free shard access.
    per_type: DashMap<String, PerTypeCounters>,

    /// Performance timing data — kept behind an async RwLock because
    /// callers that record execution times are already in async context.
    performance: Arc<RwLock<PerformanceMetrics>>,
}

impl LiveMetrics {
    pub fn new() -> Self {
        Self {
            jobs_enqueued: AtomicU64::new(0),
            jobs_completed: AtomicU64::new(0),
            jobs_failed: AtomicU64::new(0),
            jobs_retried: AtomicU64::new(0),
            jobs_canceled: AtomicU64::new(0),
            per_type: DashMap::new(),
            performance: Arc::new(RwLock::new(PerformanceMetrics::new())),
        }
    }

    // --- increment methods (synchronous, no spawns) -----------------------

    pub fn increment_jobs_enqueued(&self, job_type: &str) {
        self.jobs_enqueued.fetch_add(1, Ordering::Relaxed);
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .enqueued
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_completed(&self, job_type: &str) {
        self.jobs_completed.fetch_add(1, Ordering::Relaxed);
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .completed
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_failed(&self, job_type: &str) {
        self.jobs_failed.fetch_add(1, Ordering::Relaxed);
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .failed
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_retried(&self, job_type: &str) {
        self.jobs_retried.fetch_add(1, Ordering::Relaxed);
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .retried
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_canceled(&self, job_type: &str) {
        self.jobs_canceled.fetch_add(1, Ordering::Relaxed);
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .canceled
            .fetch_add(1, Ordering::Relaxed);
    }

    // --- global getters (synchronous) -------------------------------------

    pub fn jobs_enqueued(&self) -> u64 {
        self.jobs_enqueued.load(Ordering::Relaxed)
    }

    pub fn jobs_completed(&self) -> u64 {
        self.jobs_completed.load(Ordering::Relaxed)
    }

    pub fn jobs_failed(&self) -> u64 {
        self.jobs_failed.load(Ordering::Relaxed)
    }

    pub fn jobs_retried(&self) -> u64 {
        self.jobs_retried.load(Ordering::Relaxed)
    }

    pub fn jobs_canceled(&self) -> u64 {
        self.jobs_canceled.load(Ordering::Relaxed)
    }

    // --- per-type getters (synchronous — no .await needed) ----------------

    /// Snapshot of metrics for a specific job type.
    pub fn job_type_metrics(&self, job_type: &str) -> Option<JobTypeMetrics> {
        self.per_type.get(job_type).map(|c| c.snapshot())
    }

    /// Snapshot of metrics for every registered job type.
    pub fn all_job_type_metrics(&self) -> HashMap<String, JobTypeMetrics> {
        self.per_type
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().snapshot()))
            .collect()
    }

    // --- performance (async — timer data written from async context) -------

    /// Record job execution time
    pub async fn record_execution_time(&self, job_type: &str, duration: Duration) {
        let mut performance = self.performance.write().await;
        performance.record_execution_time(job_type, duration);
    }

    /// Get performance metrics
    pub async fn performance_metrics(&self) -> PerformanceMetrics {
        self.performance.read().await.clone()
    }
}

impl Default for LiveMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// JobTypeMetrics — read-only snapshot returned to callers
// ---------------------------------------------------------------------------

/// Point-in-time snapshot of metrics for a single job type.
#[derive(Debug, Clone, Default)]
pub struct JobTypeMetrics {
    pub enqueued: u64,
    pub completed: u64,
    pub failed: u64,
    pub retried: u64,
    pub canceled: u64,
}

impl JobTypeMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Success rate as a percentage (0.0 – 100.0).
    pub fn success_rate(&self) -> f64 {
        let total = self.completed + self.failed;
        if total == 0 {
            100.0
        } else {
            (self.completed as f64 / total as f64) * 100.0
        }
    }

    /// Retry rate as a percentage of enqueued jobs.
    pub fn retry_rate(&self) -> f64 {
        if self.enqueued == 0 {
            0.0
        } else {
            (self.retried as f64 / self.enqueued as f64) * 100.0
        }
    }
}

// ---------------------------------------------------------------------------
// PerformanceMetrics — execution timing ring-buffer
// ---------------------------------------------------------------------------

/// Per-job-type execution timing data.
///
/// Uses `VecDeque` so that evicting the oldest entry when the ring buffer is
/// full is O(1) (`pop_front`) rather than O(n) (`Vec::remove(0)`).
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    execution_times: HashMap<String, VecDeque<Duration>>,
    last_updated: DateTime<Utc>,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            execution_times: HashMap::new(),
            last_updated: Utc::now(),
        }
    }

    /// Record execution time for a job type (ring-buffer: keeps the last 1000).
    pub fn record_execution_time(&mut self, job_type: &str, duration: Duration) {
        let times = self
            .execution_times
            .entry(job_type.to_string())
            .or_default();
        times.push_back(duration);
        if times.len() > 1000 {
            times.pop_front(); // O(1) — was Vec::remove(0) which is O(n)
        }
        self.last_updated = Utc::now();
    }

    /// Average execution time for a job type.
    pub fn average_execution_time(&self, job_type: &str) -> Option<Duration> {
        let times = self.execution_times.get(job_type)?;
        if times.is_empty() {
            return None;
        }
        let total_ms: i64 = times.iter().map(|d| d.num_milliseconds()).sum();
        Some(Duration::milliseconds(total_ms / times.len() as i64))
    }

    /// Percentile execution time for a job type (e.g. 50.0 for p50).
    pub fn percentile_execution_time(&self, job_type: &str, percentile: f64) -> Option<Duration> {
        let times = self.execution_times.get(job_type)?;
        if times.is_empty() {
            return None;
        }
        let mut sorted: Vec<Duration> = times.iter().cloned().collect();
        sorted.sort_by_key(|d| d.num_milliseconds());
        let index = ((percentile / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted.get(index).cloned()
    }

    /// All job types that have timing data.
    pub fn job_types(&self) -> Vec<String> {
        self.execution_times.keys().cloned().collect()
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MetricsCollector — aggregates LiveMetrics for snapshots
// ---------------------------------------------------------------------------

/// Aggregates live metrics into point-in-time snapshots.
pub struct MetricsCollector {
    live_metrics: Arc<LiveMetrics>,
}

impl MetricsCollector {
    pub fn new(live_metrics: Arc<LiveMetrics>) -> Self {
        Self { live_metrics }
    }

    /// Collect a snapshot of all current metrics.
    pub async fn collect_snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            timestamp: Utc::now(),
            global: GlobalMetrics {
                jobs_enqueued: self.live_metrics.jobs_enqueued(),
                jobs_completed: self.live_metrics.jobs_completed(),
                jobs_failed: self.live_metrics.jobs_failed(),
                jobs_retried: self.live_metrics.jobs_retried(),
                jobs_canceled: self.live_metrics.jobs_canceled(),
            },
            // all_job_type_metrics is now synchronous — no .await needed
            job_types: self.live_metrics.all_job_type_metrics(),
            performance: self.live_metrics.performance_metrics().await,
        }
    }

    pub fn live_metrics(&self) -> &LiveMetrics {
        &self.live_metrics
    }
}

// ---------------------------------------------------------------------------
// Snapshot types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub timestamp: DateTime<Utc>,
    pub global: GlobalMetrics,
    pub job_types: HashMap<String, JobTypeMetrics>,
    pub performance: PerformanceMetrics,
}

#[derive(Debug, Clone)]
pub struct GlobalMetrics {
    pub jobs_enqueued: u64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub jobs_retried: u64,
    pub jobs_canceled: u64,
}

impl GlobalMetrics {
    pub fn success_rate(&self) -> f64 {
        let total = self.jobs_completed + self.jobs_failed;
        if total == 0 {
            100.0
        } else {
            (self.jobs_completed as f64 / total as f64) * 100.0
        }
    }

    pub fn retry_rate(&self) -> f64 {
        if self.jobs_enqueued == 0 {
            0.0
        } else {
            (self.jobs_retried as f64 / self.jobs_enqueued as f64) * 100.0
        }
    }

    pub fn jobs_in_progress(&self) -> u64 {
        self.jobs_enqueued
            .saturating_sub(self.jobs_completed + self.jobs_failed + self.jobs_canceled)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_live_metrics() {
        let metrics = LiveMetrics::new();

        metrics.increment_jobs_enqueued("test_job");
        metrics.increment_jobs_completed("test_job");

        // Global counters are immediately consistent
        assert_eq!(metrics.jobs_enqueued(), 1);
        assert_eq!(metrics.jobs_completed(), 1);

        // Per-type counters are also immediately consistent — no sleep needed
        let job_metrics = metrics.job_type_metrics("test_job").unwrap();
        assert_eq!(job_metrics.enqueued, 1);
        assert_eq!(job_metrics.completed, 1);
        assert_eq!(job_metrics.success_rate(), 100.0);
    }

    #[tokio::test]
    async fn test_performance_metrics() {
        let mut perf = PerformanceMetrics::new();

        perf.record_execution_time("test_job", Duration::milliseconds(100));
        perf.record_execution_time("test_job", Duration::milliseconds(200));
        perf.record_execution_time("test_job", Duration::milliseconds(300));

        let avg = perf.average_execution_time("test_job").unwrap();
        assert_eq!(avg.num_milliseconds(), 200);

        let p50 = perf.percentile_execution_time("test_job", 50.0).unwrap();
        assert_eq!(p50.num_milliseconds(), 200);
    }

    #[test]
    fn test_ring_buffer_eviction_is_bounded() {
        let mut perf = PerformanceMetrics::new();
        // Insert 1100 entries — ring buffer should cap at 1000
        for i in 0..1100u64 {
            perf.record_execution_time("job", Duration::milliseconds(i as i64));
        }
        assert_eq!(
            perf.execution_times["job"].len(),
            1000,
            "ring buffer must cap at 1000 entries"
        );
        // The oldest 100 should have been evicted; first entry is ms=100
        assert_eq!(
            perf.execution_times["job"][0].num_milliseconds(),
            100,
            "oldest entries are evicted first"
        );
    }

    #[test]
    fn test_global_metrics() {
        let global = GlobalMetrics {
            jobs_enqueued: 100,
            jobs_completed: 80,
            jobs_failed: 10,
            jobs_retried: 5,
            jobs_canceled: 5,
        };

        assert_eq!(global.success_rate(), 88.88888888888889);
        assert_eq!(global.retry_rate(), 5.0);
        assert_eq!(global.jobs_in_progress(), 5);
    }
}
