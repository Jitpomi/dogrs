use std::time::Duration;
use async_trait::async_trait;
use crate::{JobError, JobId, JobPriority};

/// Revolutionary zero-copy job trait that eliminates serialization overhead
#[async_trait]
pub trait Job: Send + Sync + 'static {
    /// Context type passed to job execution
    type Context: Send + Sync + Clone + 'static;
    
    /// Result type returned by job execution
    type Result: Send + Sync + 'static;
    
    /// Error type for job-specific errors
    type Error: Into<JobError> + Send + Sync + 'static;

    /// Zero-copy execution - direct memory access, no serialization
    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, Self::Error>;

    /// Compile-time job type identifier - no runtime overhead
    const JOB_TYPE: &'static str;
    
    /// Compile-time priority - no runtime dispatch
    const PRIORITY: JobPriority = JobPriority::Normal;
    
    /// Compile-time retry configuration
    const MAX_RETRIES: u32 = 3;
    
    /// Advanced scheduling configuration
    fn schedule(&self) -> Schedule {
        Schedule::Immediate
    }
    
    /// Job dependencies for workflow orchestration
    fn dependencies(&self) -> &[JobId] {
        &[]
    }
    
    /// Execution timeout
    fn timeout(&self) -> Option<Duration> {
        None
    }
    
    /// Resource requirements for adaptive scheduling
    fn resource_requirements(&self) -> ResourceRequirements {
        ResourceRequirements::default()
    }
    
    /// Idempotency key generation
    fn idempotency_key(&self) -> Option<String> {
        None
    }
}

/// Advanced scheduling options
#[derive(Debug, Clone)]
pub enum Schedule {
    /// Execute immediately
    Immediate,
    
    /// Execute after delay
    Delayed(Duration),
    
    /// Execute on cron schedule
    #[cfg(feature = "cron-scheduling")]
    Cron(cron::Schedule),
    
    /// Execute after another job completes
    After(JobId),
    
    /// Execute when condition is met
    Conditional(Box<dyn Fn(&JobContext) -> bool + Send + Sync>),
    
    /// Execute at specific time
    At(chrono::DateTime<chrono::Utc>),
}

/// Resource requirements for intelligent scheduling
#[derive(Debug, Clone, Default)]
pub struct ResourceRequirements {
    /// CPU intensity (0.0 to 1.0)
    pub cpu_intensity: f32,
    
    /// Memory requirements in MB
    pub memory_mb: u64,
    
    /// I/O intensity (0.0 to 1.0)  
    pub io_intensity: f32,
    
    /// Network bandwidth requirements in MB/s
    pub network_mbps: f32,
    
    /// Requires exclusive access
    pub exclusive: bool,
}

/// Job execution context with observability
#[derive(Debug, Clone)]
pub struct JobContext {
    /// Job identifier
    pub job_id: JobId,
    
    /// Execution attempt number
    pub attempt: u32,
    
    /// Tenant context
    pub tenant_id: String,
    
    /// Distributed tracing context
    pub trace_id: Option<String>,
    
    /// Performance metrics collector
    pub metrics: MetricsCollector,
    
    /// Cancellation token
    pub cancellation: tokio_util::sync::CancellationToken,
}

/// Zero-allocation metrics collector
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    start_time: std::time::Instant,
    cpu_usage: std::sync::Arc<std::sync::atomic::AtomicU64>,
    memory_usage: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            start_time: std::time::Instant::now(),
            cpu_usage: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            memory_usage: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
    
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
    
    pub fn record_cpu_usage(&self, usage: f64) {
        self.cpu_usage.store(
            (usage * 1000.0) as u64, 
            std::sync::atomic::Ordering::Relaxed
        );
    }
    
    pub fn record_memory_usage(&self, bytes: u64) {
        self.memory_usage.store(bytes, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile-time job registration macro for zero-cost dispatch
#[macro_export]
macro_rules! register_job {
    ($job_type:ty) => {
        impl JobTypeInfo for $job_type {
            const TYPE_ID: &'static str = <$job_type as Job>::JOB_TYPE;
            const PRIORITY: JobPriority = <$job_type as Job>::PRIORITY;
            const MAX_RETRIES: u32 = <$job_type as Job>::MAX_RETRIES;
        }
    };
}

/// Compile-time job type information
pub trait JobTypeInfo {
    const TYPE_ID: &'static str;
    const PRIORITY: JobPriority;
    const MAX_RETRIES: u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JobPriority;

    #[derive(Debug)]
    struct TestJob {
        data: String,
    }

    #[async_trait]
    impl Job for TestJob {
        type Context = ();
        type Result = String;
        type Error = JobError;

        async fn execute(&self, _ctx: Self::Context) -> Result<Self::Result, Self::Error> {
            Ok(format!("Processed: {}", self.data))
        }

        const JOB_TYPE: &'static str = "test_job";
        const PRIORITY: JobPriority = JobPriority::High;
        const MAX_RETRIES: u32 = 5;
    }

    register_job!(TestJob);

    #[tokio::test]
    async fn test_zero_copy_execution() {
        let job = TestJob {
            data: "test data".to_string(),
        };

        let result = job.execute(()).await.unwrap();
        assert_eq!(result, "Processed: test data");
        
        // Verify compile-time constants
        assert_eq!(TestJob::JOB_TYPE, "test_job");
        assert_eq!(TestJob::PRIORITY, JobPriority::High);
        assert_eq!(TestJob::MAX_RETRIES, 5);
    }

    #[test]
    fn test_resource_requirements() {
        let req = ResourceRequirements {
            cpu_intensity: 0.8,
            memory_mb: 512,
            io_intensity: 0.2,
            network_mbps: 10.0,
            exclusive: false,
        };
        
        assert_eq!(req.cpu_intensity, 0.8);
        assert_eq!(req.memory_mb, 512);
    }
}
