use std::sync::Arc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use chrono::{DateTime, Utc, Duration};

/// Live metrics collector for queue operations
pub struct LiveMetrics {
    jobs_enqueued: AtomicU64,
    jobs_completed: AtomicU64,
    jobs_failed: AtomicU64,
    jobs_retried: AtomicU64,
    jobs_canceled: AtomicU64,
    
    // Per-job-type metrics
    job_type_metrics: Arc<RwLock<HashMap<String, JobTypeMetrics>>>,
    
    // Performance metrics
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
            job_type_metrics: Arc::new(RwLock::new(HashMap::new())),
            performance: Arc::new(RwLock::new(PerformanceMetrics::new())),
        }
    }

    pub fn increment_jobs_enqueued(&self, job_type: &str) {
        self.jobs_enqueued.fetch_add(1, Ordering::Relaxed);
        
        // Update per-job-type metrics asynchronously
        let job_type = job_type.to_string();
        let metrics = self.job_type_metrics.clone();
        tokio::spawn(async move {
            let mut metrics = metrics.write().await;
            metrics.entry(job_type).or_insert_with(JobTypeMetrics::new).enqueued += 1;
        });
    }

    pub fn increment_jobs_completed(&self, job_type: &str) {
        self.jobs_completed.fetch_add(1, Ordering::Relaxed);
        
        let job_type = job_type.to_string();
        let metrics = self.job_type_metrics.clone();
        tokio::spawn(async move {
            let mut metrics = metrics.write().await;
            metrics.entry(job_type).or_insert_with(JobTypeMetrics::new).completed += 1;
        });
    }

    pub fn increment_jobs_failed(&self, job_type: &str) {
        self.jobs_failed.fetch_add(1, Ordering::Relaxed);
        
        let job_type = job_type.to_string();
        let metrics = self.job_type_metrics.clone();
        tokio::spawn(async move {
            let mut metrics = metrics.write().await;
            metrics.entry(job_type).or_insert_with(JobTypeMetrics::new).failed += 1;
        });
    }

    pub fn increment_jobs_retried(&self, job_type: &str) {
        self.jobs_retried.fetch_add(1, Ordering::Relaxed);
        
        let job_type = job_type.to_string();
        let metrics = self.job_type_metrics.clone();
        tokio::spawn(async move {
            let mut metrics = metrics.write().await;
            metrics.entry(job_type).or_insert_with(JobTypeMetrics::new).retried += 1;
        });
    }

    pub fn increment_jobs_canceled(&self, job_type: &str) {
        self.jobs_canceled.fetch_add(1, Ordering::Relaxed);
        
        let job_type = job_type.to_string();
        let metrics = self.job_type_metrics.clone();
        tokio::spawn(async move {
            let mut metrics = metrics.write().await;
            metrics.entry(job_type).or_insert_with(JobTypeMetrics::new).canceled += 1;
        });
    }

    // Getters for global metrics
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

    /// Get metrics for a specific job type
    pub async fn job_type_metrics(&self, job_type: &str) -> Option<JobTypeMetrics> {
        let metrics = self.job_type_metrics.read().await;
        metrics.get(job_type).cloned()
    }

    /// Get all job type metrics
    pub async fn all_job_type_metrics(&self) -> HashMap<String, JobTypeMetrics> {
        self.job_type_metrics.read().await.clone()
    }

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

/// Metrics for a specific job type
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

    /// Calculate success rate as percentage
    pub fn success_rate(&self) -> f64 {
        let total_processed = self.completed + self.failed;
        if total_processed == 0 {
            100.0
        } else {
            (self.completed as f64 / total_processed as f64) * 100.0
        }
    }

    /// Calculate retry rate as percentage
    pub fn retry_rate(&self) -> f64 {
        if self.enqueued == 0 {
            0.0
        } else {
            (self.retried as f64 / self.enqueued as f64) * 100.0
        }
    }
}

/// Performance metrics for job execution
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    execution_times: HashMap<String, Vec<Duration>>,
    last_updated: DateTime<Utc>,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            execution_times: HashMap::new(),
            last_updated: Utc::now(),
        }
    }

    /// Record execution time for a job type
    pub fn record_execution_time(&mut self, job_type: &str, duration: Duration) {
        let times = self.execution_times.entry(job_type.to_string()).or_default();
        times.push(duration);
        
        // Keep only last 1000 measurements per job type
        if times.len() > 1000 {
            times.remove(0);
        }
        
        self.last_updated = Utc::now();
    }

    /// Get average execution time for a job type
    pub fn average_execution_time(&self, job_type: &str) -> Option<Duration> {
        let times = self.execution_times.get(job_type)?;
        if times.is_empty() {
            return None;
        }

        let total_ms: i64 = times.iter().map(|d| d.num_milliseconds()).sum();
        let avg_ms = total_ms / times.len() as i64;
        Some(Duration::milliseconds(avg_ms))
    }

    /// Get percentile execution time for a job type
    pub fn percentile_execution_time(&self, job_type: &str, percentile: f64) -> Option<Duration> {
        let times = self.execution_times.get(job_type)?;
        if times.is_empty() {
            return None;
        }

        let mut sorted_times = times.clone();
        sorted_times.sort_by_key(|d| d.num_milliseconds());
        
        let index = ((percentile / 100.0) * (sorted_times.len() - 1) as f64).round() as usize;
        sorted_times.get(index).cloned()
    }

    /// Get all job types with performance data
    pub fn job_types(&self) -> Vec<String> {
        self.execution_times.keys().cloned().collect()
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics collector that aggregates data from multiple sources
pub struct MetricsCollector {
    live_metrics: Arc<LiveMetrics>,
}

impl MetricsCollector {
    pub fn new(live_metrics: Arc<LiveMetrics>) -> Self {
        Self { live_metrics }
    }

    /// Collect current snapshot of all metrics
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
            job_types: self.live_metrics.all_job_type_metrics().await,
            performance: self.live_metrics.performance_metrics().await,
        }
    }

    /// Get live metrics reference
    pub fn live_metrics(&self) -> &LiveMetrics {
        &self.live_metrics
    }
}

/// Snapshot of metrics at a point in time
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub timestamp: DateTime<Utc>,
    pub global: GlobalMetrics,
    pub job_types: HashMap<String, JobTypeMetrics>,
    pub performance: PerformanceMetrics,
}

/// Global queue metrics
#[derive(Debug, Clone)]
pub struct GlobalMetrics {
    pub jobs_enqueued: u64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub jobs_retried: u64,
    pub jobs_canceled: u64,
}

impl GlobalMetrics {
    /// Calculate overall success rate
    pub fn success_rate(&self) -> f64 {
        let total_processed = self.jobs_completed + self.jobs_failed;
        if total_processed == 0 {
            100.0
        } else {
            (self.jobs_completed as f64 / total_processed as f64) * 100.0
        }
    }

    /// Calculate overall retry rate
    pub fn retry_rate(&self) -> f64 {
        if self.jobs_enqueued == 0 {
            0.0
        } else {
            (self.jobs_retried as f64 / self.jobs_enqueued as f64) * 100.0
        }
    }

    /// Calculate jobs in progress
    pub fn jobs_in_progress(&self) -> u64 {
        self.jobs_enqueued.saturating_sub(self.jobs_completed + self.jobs_failed + self.jobs_canceled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_live_metrics() {
        let metrics = LiveMetrics::new();
        
        metrics.increment_jobs_enqueued("test_job");
        metrics.increment_jobs_completed("test_job");
        
        assert_eq!(metrics.jobs_enqueued(), 1);
        assert_eq!(metrics.jobs_completed(), 1);
        
        // Give async tasks time to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        let job_metrics = metrics.job_type_metrics("test_job").await.unwrap();
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
    fn test_global_metrics() {
        let global = GlobalMetrics {
            jobs_enqueued: 100,
            jobs_completed: 80,
            jobs_failed: 10,
            jobs_retried: 5,
            jobs_canceled: 5,
        };
        
        assert_eq!(global.success_rate(), 88.88888888888889); // 80/(80+10) * 100
        assert_eq!(global.retry_rate(), 5.0); // 5/100 * 100
        assert_eq!(global.jobs_in_progress(), 5); // 100 - 80 - 10 - 5
    }
}
