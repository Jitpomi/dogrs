use std::sync::Arc;
use std::time::{Duration, Instant};
use async_trait::async_trait;
use tokio::sync::{Semaphore, RwLock};
use dashmap::DashMap;
use tracing::{info, warn, error, instrument};

use crate::{
    QueueResult, QueueError, JobError, JobId,
    execution::{Job, JobContext, MetricsCollector, ResourceRequirements},
    observability::{LiveMetrics, PerformanceMetrics},
};

/// Revolutionary zero-copy job executor with adaptive intelligence
pub struct JobExecutor {
    /// Execution semaphore for concurrency control
    semaphore: Arc<Semaphore>,
    
    /// Active job tracking for observability
    active_jobs: Arc<DashMap<JobId, ExecutionInfo>>,
    
    /// Performance metrics collector
    metrics: Arc<RwLock<LiveMetrics>>,
    
    /// Resource usage tracker
    resource_tracker: Arc<ResourceTracker>,
}

impl JobExecutor {
    /// Create a new job executor with specified concurrency
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            active_jobs: Arc::new(DashMap::new()),
            metrics: Arc::new(RwLock::new(LiveMetrics::new())),
            resource_tracker: Arc::new(ResourceTracker::new()),
        }
    }

    /// Execute a job with zero-copy optimization and full observability
    #[instrument(skip(self, job, context), fields(job_id = %context.job_id, job_type = J::JOB_TYPE))]
    pub async fn execute<J: Job>(
        &self,
        job: J,
        context: ExecutionContext<J::Context>,
    ) -> QueueResult<J::Result> {
        // Acquire execution permit (adaptive concurrency control)
        let _permit = self.semaphore.acquire().await
            .map_err(|_| QueueError::Internal("Failed to acquire execution permit".to_string()))?;

        let job_id = context.job_context.job_id.clone();
        let start_time = Instant::now();
        
        // Track job execution
        let execution_info = ExecutionInfo {
            job_type: J::JOB_TYPE.to_string(),
            start_time,
            resource_requirements: job.resource_requirements(),
        };
        
        self.active_jobs.insert(job_id.clone(), execution_info);

        // Execute with comprehensive error handling and metrics
        let result = self.execute_with_monitoring(job, context).await;
        
        // Clean up tracking
        self.active_jobs.remove(&job_id);
        
        // Update performance metrics
        self.update_metrics(J::JOB_TYPE, start_time.elapsed(), &result).await;
        
        result
    }

    /// Execute job with comprehensive monitoring and resource tracking
    async fn execute_with_monitoring<J: Job>(
        &self,
        job: J,
        context: ExecutionContext<J::Context>,
    ) -> QueueResult<J::Result> {
        let job_context = &context.job_context;
        let user_context = context.user_context;
        
        // Check for cancellation before execution
        if job_context.cancellation.is_cancelled() {
            return Err(QueueError::JobCanceled);
        }

        // Record resource usage start
        let resource_snapshot = self.resource_tracker.snapshot();
        
        // Execute with timeout if specified
        let execution_future = job.execute(user_context);
        
        let result = match job.timeout() {
            Some(timeout) => {
                tokio::time::timeout(timeout, execution_future)
                    .await
                    .map_err(|_| QueueError::Internal("Job execution timeout".to_string()))?
                    .map_err(|e| QueueError::Internal(format!("Job execution failed: {}", e.into())))
            }
            None => {
                execution_future
                    .await
                    .map_err(|e| QueueError::Internal(format!("Job execution failed: {}", e.into())))
            }
        };

        // Record resource usage end
        self.resource_tracker.record_usage(
            J::JOB_TYPE,
            resource_snapshot,
            job_context.metrics.elapsed(),
        );

        result
    }

    /// Update performance metrics with execution results
    async fn update_metrics(
        &self,
        job_type: &str,
        duration: Duration,
        result: &QueueResult<impl std::fmt::Debug>,
    ) {
        let mut metrics = self.metrics.write().await;
        
        metrics.record_execution(job_type, duration, result.is_ok());
        
        if result.is_err() {
            warn!("Job execution failed for type: {}", job_type);
        } else {
            info!("Job executed successfully for type: {} in {:?}", job_type, duration);
        }
    }

    /// Get current performance metrics
    pub async fn get_metrics(&self) -> LiveMetrics {
        self.metrics.read().await.clone()
    }

    /// Get active job count
    pub fn active_job_count(&self) -> usize {
        self.active_jobs.len()
    }

    /// Get available execution slots
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

/// Execution context containing both job and user contexts
pub struct ExecutionContext<C> {
    /// Job execution context with observability
    pub job_context: JobContext,
    
    /// User-defined context
    pub user_context: C,
}

impl<C> ExecutionContext<C> {
    pub fn new(job_context: JobContext, user_context: C) -> Self {
        Self {
            job_context,
            user_context,
        }
    }
}

/// Information about a currently executing job
#[derive(Debug, Clone)]
struct ExecutionInfo {
    job_type: String,
    start_time: Instant,
    resource_requirements: ResourceRequirements,
}

/// Resource usage tracker for adaptive scheduling
#[derive(Debug)]
pub struct ResourceTracker {
    /// CPU usage history
    cpu_history: Arc<RwLock<Vec<f64>>>,
    
    /// Memory usage history
    memory_history: Arc<RwLock<Vec<u64>>>,
    
    /// Job type performance profiles
    job_profiles: Arc<DashMap<String, JobProfile>>,
}

impl ResourceTracker {
    pub fn new() -> Self {
        Self {
            cpu_history: Arc::new(RwLock::new(Vec::new())),
            memory_history: Arc::new(RwLock::new(Vec::new())),
            job_profiles: Arc::new(DashMap::new()),
        }
    }

    /// Take a resource usage snapshot
    pub fn snapshot(&self) -> ResourceSnapshot {
        ResourceSnapshot {
            timestamp: Instant::now(),
            cpu_usage: self.get_current_cpu_usage(),
            memory_usage: self.get_current_memory_usage(),
        }
    }

    /// Record resource usage for a job execution
    pub fn record_usage(
        &self,
        job_type: &str,
        start_snapshot: ResourceSnapshot,
        duration: Duration,
    ) {
        let end_snapshot = self.snapshot();
        
        let cpu_delta = end_snapshot.cpu_usage - start_snapshot.cpu_usage;
        let memory_delta = end_snapshot.memory_usage.saturating_sub(start_snapshot.memory_usage);
        
        // Update job profile
        let mut profile = self.job_profiles
            .entry(job_type.to_string())
            .or_insert_with(JobProfile::new);
        
        profile.update(duration, cpu_delta, memory_delta);
    }

    /// Get current CPU usage (placeholder - would integrate with system metrics)
    fn get_current_cpu_usage(&self) -> f64 {
        // In a real implementation, this would read from /proc/stat or similar
        0.0
    }

    /// Get current memory usage (placeholder - would integrate with system metrics)
    fn get_current_memory_usage(&self) -> u64 {
        // In a real implementation, this would read from /proc/meminfo or similar
        0
    }

    /// Get performance profile for a job type
    pub fn get_job_profile(&self, job_type: &str) -> Option<JobProfile> {
        self.job_profiles.get(job_type).map(|p| p.clone())
    }
}

/// Resource usage snapshot
#[derive(Debug, Clone)]
pub struct ResourceSnapshot {
    timestamp: Instant,
    cpu_usage: f64,
    memory_usage: u64,
}

/// Performance profile for a job type
#[derive(Debug, Clone)]
pub struct JobProfile {
    /// Average execution duration
    pub avg_duration: Duration,
    
    /// Average CPU usage
    pub avg_cpu: f64,
    
    /// Average memory usage
    pub avg_memory: u64,
    
    /// Execution count
    pub execution_count: u64,
    
    /// Success rate
    pub success_rate: f64,
}

impl JobProfile {
    pub fn new() -> Self {
        Self {
            avg_duration: Duration::ZERO,
            avg_cpu: 0.0,
            avg_memory: 0,
            execution_count: 0,
            success_rate: 1.0,
        }
    }

    /// Update profile with new execution data
    pub fn update(&mut self, duration: Duration, cpu_usage: f64, memory_usage: u64) {
        let count = self.execution_count as f64;
        
        // Update averages using exponential moving average
        let alpha = if count < 10.0 { 1.0 / (count + 1.0) } else { 0.1 };
        
        self.avg_duration = Duration::from_nanos(
            ((1.0 - alpha) * self.avg_duration.as_nanos() as f64 + 
             alpha * duration.as_nanos() as f64) as u64
        );
        
        self.avg_cpu = (1.0 - alpha) * self.avg_cpu + alpha * cpu_usage;
        self.avg_memory = ((1.0 - alpha) * self.avg_memory as f64 + alpha * memory_usage as f64) as u64;
        
        self.execution_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobPriority, execution::Job};

    #[derive(Debug)]
    struct TestJob {
        duration: Duration,
    }

    #[async_trait]
    impl Job for TestJob {
        type Context = ();
        type Result = String;
        type Error = JobError;

        async fn execute(&self, _ctx: Self::Context) -> Result<Self::Result, Self::Error> {
            tokio::time::sleep(self.duration).await;
            Ok("completed".to_string())
        }

        const JOB_TYPE: &'static str = "test_job";
        const PRIORITY: JobPriority = JobPriority::Normal;

        fn timeout(&self) -> Option<Duration> {
            Some(Duration::from_secs(5))
        }
    }

    #[tokio::test]
    async fn test_job_executor() {
        let executor = JobExecutor::new(2);
        
        let job = TestJob {
            duration: Duration::from_millis(100),
        };
        
        let job_context = JobContext {
            job_id: JobId::new(),
            attempt: 1,
            tenant_id: "test".to_string(),
            trace_id: None,
            metrics: MetricsCollector::new(),
            cancellation: tokio_util::sync::CancellationToken::new(),
        };
        
        let context = ExecutionContext::new(job_context, ());
        
        let result = executor.execute(job, context).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "completed");
    }

    #[tokio::test]
    async fn test_concurrent_execution() {
        let executor = Arc::new(JobExecutor::new(2));
        
        let mut handles = Vec::new();
        
        for i in 0..5 {
            let executor = executor.clone();
            let handle = tokio::spawn(async move {
                let job = TestJob {
                    duration: Duration::from_millis(50),
                };
                
                let job_context = JobContext {
                    job_id: JobId::from(format!("job_{}", i)),
                    attempt: 1,
                    tenant_id: "test".to_string(),
                    trace_id: None,
                    metrics: MetricsCollector::new(),
                    cancellation: tokio_util::sync::CancellationToken::new(),
                };
                
                let context = ExecutionContext::new(job_context, ());
                executor.execute(job, context).await
            });
            
            handles.push(handle);
        }
        
        // Wait for all jobs to complete
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }
        
        // Verify metrics were collected
        let metrics = executor.get_metrics().await;
        assert!(metrics.total_executions() > 0);
    }

    #[test]
    fn test_resource_tracker() {
        let tracker = ResourceTracker::new();
        
        let snapshot = tracker.snapshot();
        assert!(snapshot.timestamp.elapsed() < Duration::from_millis(10));
        
        tracker.record_usage("test_job", snapshot, Duration::from_millis(100));
        
        let profile = tracker.get_job_profile("test_job");
        assert!(profile.is_some());
        
        let profile = profile.unwrap();
        assert_eq!(profile.execution_count, 1);
    }
}
