use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
/// Per-job-type counters use a `DashMap` of per-entry `AtomicU64`s so that
/// `increment_*` methods are completely synchronous — no `tokio::spawn`,
/// no lock contention, no ordering surprises.
///
/// Global totals (e.g. [`Self::jobs_enqueued`]) are computed by summing over
/// per-type entries at read time rather than maintaining a redundant global
/// `AtomicU64`. This eliminates the two-phase write window (global counter
/// advanced before per-type, or vice-versa) that could produce inconsistent
/// snapshots. The O(n_job_types) read cost is acceptable for observability.
pub struct LiveMetrics {
    /// Per-job-type counters. DashMap gives lock-free shard access.
    per_type: DashMap<String, PerTypeCounters>,

    /// Performance timing data — kept behind a `std::sync::Mutex` because
    /// `record_execution_time` is a synchronous write (VecDeque push + optional
    /// pop_front — nanoseconds). Using a tokio async lock would add an unnecessary
    /// yield point on every job completion.
    performance: Arc<Mutex<PerformanceMetrics>>,
}

impl LiveMetrics {
    pub fn new() -> Self {
        Self {
            per_type: DashMap::new(),
            performance: Arc::new(Mutex::new(PerformanceMetrics::new())),
        }
    }

    // --- increment methods (synchronous, no spawns) -----------------------

    pub fn increment_jobs_enqueued(&self, job_type: &str) {
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .enqueued
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_completed(&self, job_type: &str) {
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .completed
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_failed(&self, job_type: &str) {
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .failed
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_retried(&self, job_type: &str) {
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .retried
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_jobs_canceled(&self, job_type: &str) {
        self.per_type
            .entry(job_type.to_string())
            .or_default()
            .canceled
            .fetch_add(1, Ordering::Relaxed);
    }

    // --- global getters: derived by summing per-type (no separate AtomicU64) ---
    //
    // Summing at read time guarantees that global totals are always consistent
    // with per-type breakdown in a single snapshot — there is no window where
    // the global counter is ahead or behind.

    pub fn jobs_enqueued(&self) -> u64 {
        self.per_type.iter().map(|e| e.enqueued.load(Ordering::Relaxed)).sum()
    }

    pub fn jobs_completed(&self) -> u64 {
        self.per_type.iter().map(|e| e.completed.load(Ordering::Relaxed)).sum()
    }

    pub fn jobs_failed(&self) -> u64 {
        self.per_type.iter().map(|e| e.failed.load(Ordering::Relaxed)).sum()
    }

    pub fn jobs_retried(&self) -> u64 {
        self.per_type.iter().map(|e| e.retried.load(Ordering::Relaxed)).sum()
    }

    pub fn jobs_canceled(&self) -> u64 {
        self.per_type.iter().map(|e| e.canceled.load(Ordering::Relaxed)).sum()
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
    pub fn record_execution_time(&self, job_type: &str, duration: Duration) {
        self.performance
            .lock()
            .expect("performance metrics lock poisoned")
            .record_execution_time(job_type, duration);
    }

    /// Get performance metrics snapshot (synchronous — Mutex, not async lock).
    pub fn performance_metrics(&self) -> PerformanceMetrics {
        self.performance
            .lock()
            .expect("performance metrics lock poisoned")
            .clone()
    }
    /// Coherent single-pass snapshot of all metrics.
    ///
    /// Iterates the per-type DashMap exactly once, accumulating both the global
    /// totals and the per-type breakdown from the same set of atomic reads.
    /// This eliminates the inconsistency window present when five independent
    /// `jobs_*()` calls are made sequentially: between calls, a job may complete
    /// and advance `completed` while `enqueued` was already captured, making
    /// derived invariants (e.g. `enqueued ≥ completed + failed + in_flight`)
    /// appear violated in a single snapshot.
    pub fn snapshot_all(&self) -> (GlobalMetrics, std::collections::HashMap<String, JobTypeMetrics>) {
        let mut global = GlobalMetrics::default();
        let mut per_type = std::collections::HashMap::new();
        for entry in self.per_type.iter() {
            let m = entry.value().snapshot();
            global.jobs_enqueued  += m.enqueued;
            global.jobs_completed += m.completed;
            global.jobs_failed    += m.failed;
            global.jobs_retried   += m.retried;
            global.jobs_canceled  += m.canceled;
            per_type.insert(entry.key().clone(), m);
        }
        (global, per_type)
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
    ///
    /// Returns `0.0` when no jobs have completed or failed — "no data" must
    /// not be reported as a perfect record. Callers that want to distinguish
    /// "no data" from "0% success" should check `completed + failed == 0` first.
    pub fn success_rate(&self) -> f64 {
        let total = self.completed + self.failed;
        if total == 0 {
            0.0 // no data — not 100%
        } else {
            (self.completed as f64 / total as f64) * 100.0
        }
    }

    /// Retry event rate: retry events per terminal job (completed + failed).
    ///
    /// A job retried `n` times contributes `n` to `retried` but only 1 to `enqueued`,
    /// so dividing by `enqueued` yields values above 100%. The correct denominator
    /// is `completed + failed` (terminal jobs = total original job attempts).
    pub fn retry_rate(&self) -> f64 {
        let terminal = self.completed + self.failed;
        if terminal == 0 {
            0.0
        } else {
            (self.retried as f64 / terminal as f64) * 100.0
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
///
/// Stores `std::time::Duration` (always non-negative, no chrono dependency)
/// rather than `chrono::Duration` (signed, allows negative values).
///
/// `execution_times` is wrapped in `Arc` so that `Clone` (called by
/// `LiveMetrics::performance_metrics()` on every snapshot) is O(1) — it
/// only increments the Arc refcount rather than deep-copying all ring buffers.
/// Copy-on-write semantics are preserved: `Arc::make_mut` in
/// `record_execution_time` detects when a snapshot holder still holds a
/// reference and performs a deep clone only at that point, not on every
/// snapshot.
pub struct PerformanceMetrics {
    execution_times: Arc<HashMap<String, VecDeque<Duration>>>,
    last_updated: DateTime<Utc>,
}

impl Clone for PerformanceMetrics {
    /// O(1) clone — shares the underlying `Arc<HashMap<...>>` with the original.
    ///
    /// A subsequent `record_execution_time` call will use `Arc::make_mut` to
    /// transparently deep-clone the data only if the caller still holds this snapshot,
    /// keeping the common case (no concurrent reader) allocation-free.
    fn clone(&self) -> Self {
        Self {
            execution_times: Arc::clone(&self.execution_times),
            last_updated: self.last_updated,
        }
    }
}

impl std::fmt::Debug for PerformanceMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PerformanceMetrics")
            .field("job_types", &self.execution_times.keys().collect::<Vec<_>>())
            .field("last_updated", &self.last_updated)
            .finish()
    }
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            execution_times: Arc::new(HashMap::new()),
            last_updated: Utc::now(),
        }
    }

    /// Record execution time for a job type (ring-buffer: keeps the last 1000).
    ///
    /// Uses `Arc::make_mut` on `execution_times` for copy-on-write semantics:
    /// if a snapshot caller is still holding a reference, only then is the
    /// backing `HashMap` deep-cloned. The common case (no concurrent snapshot)
    /// mutates in place without any allocation.
    pub fn record_execution_time(&mut self, job_type: &str, duration: Duration) {
        let times = Arc::make_mut(&mut self.execution_times)
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
        let total_nanos: u128 = times.iter().map(|d| d.as_nanos()).sum();
        Some(Duration::from_nanos((total_nanos / times.len() as u128) as u64))
    }

    /// Percentile execution time for a job type (e.g. 50.0 for p50).
    ///
    /// Returns `None` if there is no timing data for the job type or if
    /// `percentile` is outside `[0.0, 100.0]`.
    pub fn percentile_execution_time(&self, job_type: &str, percentile: f64) -> Option<Duration> {
        self.percentiles(job_type, &[percentile])
            .into_iter()
            .next()
            .flatten()
    }

    /// Compute multiple percentiles in a single sort pass.
    ///
    /// More efficient than calling [`percentile_execution_time`] repeatedly when
    /// several quantiles are needed at once (e.g. p50 + p95 + p99 for dashboards).
    ///
    /// Returns one `Option<Duration>` per input percentile, in the same order.
    /// Returns `None` for a given percentile if:
    /// - There is no timing data for the job type.
    /// - The percentile value is outside `[0.0, 100.0]` (programming error — callers
    ///   should validate before calling).
    pub fn percentiles(&self, job_type: &str, percentiles: &[f64]) -> Vec<Option<Duration>> {
        let times = match self.execution_times.get(job_type) {
            Some(t) if !t.is_empty() => t,
            _ => return vec![None; percentiles.len()],
        };
        // Sort once for all requested percentiles.
        // std::time::Duration implements Ord — sort_unstable is correct and faster.
        let mut sorted: Vec<Duration> = times.iter().cloned().collect();
        sorted.sort_unstable();
        let n = sorted.len();
        percentiles
            .iter()
            .map(|&p| {
                // Validate range — a percentile outside [0, 100] is a programming error;
                // return None rather than an out-of-bounds or misleading in-bounds index.
                if !(0.0..=100.0).contains(&p) {
                    return None;
                }
                let index = ((p / 100.0) * (n - 1) as f64).round() as usize;
                sorted.get(index).cloned()
            })
            .collect()
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

    /// Collect a coherent point-in-time snapshot of all metrics.
    ///
    /// Uses [`LiveMetrics::snapshot_all`] to traverse the per-type DashMap
    /// exactly once, so global totals and per-type breakdown are read from
    /// the same atomic loads — no inconsistency window.
    pub fn collect_snapshot(&self) -> MetricsSnapshot {
        let (global, job_types) = self.live_metrics.snapshot_all();
        MetricsSnapshot {
            timestamp: Utc::now(),
            global,
            job_types,
            performance: self.live_metrics.performance_metrics(),
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

#[derive(Debug, Clone, Default)]
pub struct GlobalMetrics {
    pub jobs_enqueued: u64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub jobs_retried: u64,
    pub jobs_canceled: u64,
}

impl GlobalMetrics {
    /// Success rate as a percentage (0.0 – 100.0).
    ///
    /// Returns `0.0` when no jobs have completed or failed.
    pub fn success_rate(&self) -> f64 {
        let total = self.jobs_completed + self.jobs_failed;
        if total == 0 {
            0.0 // no data — not 100%
        } else {
            (self.jobs_completed as f64 / total as f64) * 100.0
        }
    }

    /// Retry event rate: retry events per terminal job (completed + failed).
    ///
    /// A job retried `n` times contributes `n` to `jobs_retried` but only 1 to
    /// `jobs_enqueued`, so dividing by `jobs_enqueued` yields values above 100%.
    /// The correct denominator is `jobs_completed + jobs_failed`.
    pub fn retry_rate(&self) -> f64 {
        let terminal = self.jobs_completed + self.jobs_failed;
        if terminal == 0 {
            0.0
        } else {
            (self.jobs_retried as f64 / terminal as f64) * 100.0
        }
    }

    /// Jobs not yet in a terminal state (Enqueued + Processing + Retrying).
    ///
    /// **Note**: this includes jobs waiting in the queue (`Enqueued`) and jobs
    /// waiting for their retry delay (`Retrying`), not just jobs that are
    /// actively executing (`Processing`). An overloaded system with a large
    /// backlog reports a high value here while the true "actively executing"
    /// count equals `max_workers` at most.
    ///
    /// For a true active-worker count, query the backend for records in
    /// `JobStatus::Processing` state.
    pub fn jobs_not_yet_terminal(&self) -> u64 {
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

        perf.record_execution_time("test_job", Duration::from_millis(100));
        perf.record_execution_time("test_job", Duration::from_millis(200));
        perf.record_execution_time("test_job", Duration::from_millis(300));

        let avg = perf.average_execution_time("test_job").unwrap();
        assert_eq!(avg.as_millis(), 200);

        let p50 = perf.percentile_execution_time("test_job", 50.0).unwrap();
        assert_eq!(p50.as_millis(), 200);
    }

    #[test]
    fn test_ring_buffer_eviction_is_bounded() {
        let mut perf = PerformanceMetrics::new();
        // Insert 1100 entries — ring buffer should cap at 1000
        for i in 0..1100u64 {
            perf.record_execution_time("job", Duration::from_millis(i));
        }
        assert_eq!(
            perf.execution_times["job"].len(),
            1000,
            "ring buffer must cap at 1000 entries"
        );
        // The oldest 100 should have been evicted; first entry is ms=100
        assert_eq!(
            perf.execution_times["job"][0].as_millis(),
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
        // New formula: retried / (completed + failed) = 5 / (80 + 10) ≈ 5.556%
        let expected_retry_rate = 100.0 * 5.0_f64 / 90.0_f64;
        assert!(
            (global.retry_rate() - expected_retry_rate).abs() < 1e-10,
            "retry_rate expected ≈{:.4} but got {}", expected_retry_rate, global.retry_rate()
        );
        assert_eq!(global.jobs_not_yet_terminal(), 5);
    }
}

// ---------------------------------------------------------------------------
// PrometheusExporter — Prometheus text exposition format export
// ---------------------------------------------------------------------------

/// Exports [`LiveMetrics`] in [Prometheus text exposition format][fmt].
///
/// Wraps a shared [`LiveMetrics`] instance and renders all per-job-type
/// counters on each [`gather`](Self::gather) call.  The output is suitable
/// for a `/metrics` HTTP endpoint consumed by Prometheus, Grafana Mimir, or
/// VictoriaMetrics.
///
/// All counters carry a `job_type` label for per-type breakdown.  Label
/// values are escaped per the Prometheus specification (backslash and
/// double-quote are the only characters that require escaping).
///
/// Available when the `metrics` feature is enabled.
///
/// [fmt]: https://prometheus.io/docs/instrumenting/exposition_formats/
#[cfg(feature = "metrics")]
pub struct PrometheusExporter {
    live_metrics: Arc<LiveMetrics>,
}

#[cfg(feature = "metrics")]
impl PrometheusExporter {
    /// Create a new exporter from a shared [`LiveMetrics`] instance.
    pub fn new(live_metrics: Arc<LiveMetrics>) -> Self {
        Self { live_metrics }
    }

    /// Render all current metrics in Prometheus text exposition format.
    ///
    /// Uses [`LiveMetrics::snapshot_all`] to traverse the per-type DashMap
    /// exactly once — both the global totals and per-type labels are drawn
    /// from the same coherent snapshot.
    ///
    /// # Format
    ///
    /// Each metric family is rendered as:
    /// ```text
    /// # HELP <name> <help_text>
    /// # TYPE <name> counter
    /// <name>{job_type="<type>"} <value>
    /// ```
    pub fn gather(&self) -> String {
        use std::fmt::Write as _;

        let (_global, per_type) = self.live_metrics.snapshot_all();

        // Pre-allocate: ~120 bytes per line × 5 families × n job types
        let capacity = per_type.len().max(1) * 5 * 120;
        let mut out = String::with_capacity(capacity);

        /// Descriptor for a single Prometheus counter metric.
        struct Family {
            name: &'static str,
            help: &'static str,
            get: fn(&JobTypeMetrics) -> u64,
        }

        let families: &[Family] = &[
            Family {
                name: "dog_queue_jobs_enqueued_total",
                help: "Total jobs enqueued, partitioned by job type.",
                get: |m| m.enqueued,
            },
            Family {
                name: "dog_queue_jobs_completed_total",
                help: "Total jobs completed, partitioned by job type.",
                get: |m| m.completed,
            },
            Family {
                name: "dog_queue_jobs_failed_total",
                help: "Total jobs failed, partitioned by job type.",
                get: |m| m.failed,
            },
            Family {
                name: "dog_queue_jobs_retried_total",
                help: "Total retry events, partitioned by job type.",
                get: |m| m.retried,
            },
            Family {
                name: "dog_queue_jobs_canceled_total",
                help: "Total jobs canceled, partitioned by job type.",
                get: |m| m.canceled,
            },
        ];

        for family in families {
            let _ = writeln!(out, "# HELP {} {}", family.name, family.help);
            let _ = writeln!(out, "# TYPE {} counter", family.name);
            for (job_type, metrics) in &per_type {
                // Escape per Prometheus text format spec:
                // backslash → \\, double-quote → \"
                let escaped = job_type
                    .replace('\\', r"\\")
                    .replace('"', "\\\"");
                let _ = writeln!(
                    out,
                    "{}{{job_type=\"{}\"}} {}",
                    family.name,
                    escaped,
                    (family.get)(metrics),
                );
            }
        }

        out
    }
}

#[cfg(all(test, feature = "metrics"))]
mod prometheus_tests {
    use super::*;

    #[test]
    fn test_prometheus_exporter_renders_valid_text() {
        let metrics = Arc::new(LiveMetrics::new());
        metrics.increment_jobs_enqueued("send_email");
        metrics.increment_jobs_enqueued("send_email");
        metrics.increment_jobs_completed("send_email");
        metrics.increment_jobs_failed("resize_image");

        let exporter = PrometheusExporter::new(metrics);
        let output = exporter.gather();

        assert!(output.contains("# HELP dog_queue_jobs_enqueued_total"));
        assert!(output.contains("# TYPE dog_queue_jobs_enqueued_total counter"));
        assert!(output.contains(r#"dog_queue_jobs_enqueued_total{job_type="send_email"} 2"#));
        assert!(output.contains(r#"dog_queue_jobs_failed_total{job_type="resize_image"} 1"#));
    }

    #[test]
    fn test_prometheus_exporter_escapes_label_values() {
        let metrics = Arc::new(LiveMetrics::new());
        // job_type with special characters that need escaping
        metrics.increment_jobs_enqueued(r#"my\"tricky\type"#);

        let exporter = PrometheusExporter::new(metrics);
        let output = exporter.gather();

        // Backslash should be doubled, double-quote should be escaped
        assert!(output.contains(r#"job_type="my\\\"tricky\\type""#));
    }
}
