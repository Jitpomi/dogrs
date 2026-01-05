use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};
use dashmap::DashMap;
use tracing::{info, warn, debug, instrument};

use crate::{
    QueueResult, QueueError,
    execution::{JobExecutor, ExecutionContext, ResourceTracker, JobProfile},
    observability::{LiveMetrics, PerformanceMetrics},
};

/// Revolutionary adaptive executor that dynamically scales based on system load
pub struct AdaptiveExecutor {
    /// Base job executor
    executor: JobExecutor,
    
    /// Concurrency controller for intelligent scaling
    concurrency_controller: Arc<ConcurrencyController>,
    
    /// Backpressure detector
    backpressure_detector: Arc<BackpressureDetector>,
    
    /// Performance optimizer
    performance_optimizer: Arc<PerformanceOptimizer>,
}

impl AdaptiveExecutor {
    /// Create a new adaptive executor with intelligent defaults
    pub fn new() -> Self {
        let initial_concurrency = num_cpus::get().max(4);
        
        Self {
            executor: JobExecutor::new(initial_concurrency),
            concurrency_controller: Arc::new(ConcurrencyController::new(initial_concurrency)),
            backpressure_detector: Arc::new(BackpressureDetector::new()),
            performance_optimizer: Arc::new(PerformanceOptimizer::new()),
        }
    }

    /// Create adaptive executor with custom configuration
    pub fn with_config(config: AdaptiveConfig) -> Self {
        Self {
            executor: JobExecutor::new(config.initial_concurrency),
            concurrency_controller: Arc::new(ConcurrencyController::with_config(config.concurrency_config)),
            backpressure_detector: Arc::new(BackpressureDetector::with_config(config.backpressure_config)),
            performance_optimizer: Arc::new(PerformanceOptimizer::with_config(config.optimization_config)),
        }
    }

    /// Execute job with adaptive intelligence
    #[instrument(skip(self, job, context))]
    pub async fn execute_adaptive<J: crate::execution::Job>(
        &self,
        job: J,
        context: ExecutionContext<J::Context>,
    ) -> QueueResult<J::Result> {
        // Check system load and adjust concurrency
        self.adjust_concurrency().await?;
        
        // Detect backpressure and apply throttling if needed
        self.handle_backpressure().await?;
        
        // Execute with performance monitoring
        let start_time = Instant::now();
        let result = self.executor.execute(job, context).await;
        let execution_time = start_time.elapsed();
        
        // Feed performance data to optimizer
        self.performance_optimizer.record_execution(
            J::JOB_TYPE,
            execution_time,
            result.is_ok(),
        ).await;
        
        result
    }

    /// Dynamically adjust concurrency based on system metrics
    async fn adjust_concurrency(&self) -> QueueResult<()> {
        let system_load = self.get_system_load().await;
        let queue_depth = self.get_queue_depth().await;
        let current_concurrency = self.concurrency_controller.current_concurrency().await;
        
        let optimal_concurrency = self.concurrency_controller
            .calculate_optimal_concurrency(system_load, queue_depth, current_concurrency)
            .await;
        
        if optimal_concurrency != current_concurrency {
            info!(
                "Adjusting concurrency from {} to {} (load: {:.2}, queue_depth: {})",
                current_concurrency, optimal_concurrency, system_load, queue_depth
            );
            
            self.concurrency_controller
                .set_concurrency(optimal_concurrency)
                .await?;
        }
        
        Ok(())
    }

    /// Handle backpressure by applying intelligent throttling
    async fn handle_backpressure(&self) -> QueueResult<()> {
        let backpressure_level = self.backpressure_detector.detect_backpressure().await;
        
        if backpressure_level > 0.8 {
            warn!("High backpressure detected: {:.2}", backpressure_level);
            
            // Apply exponential backoff
            let delay = Duration::from_millis((backpressure_level * 1000.0) as u64);
            tokio::time::sleep(delay).await;
        }
        
        Ok(())
    }

    /// Get current system load (CPU, memory, I/O)
    async fn get_system_load(&self) -> f64 {
        // In a real implementation, this would read system metrics
        // For now, simulate based on active jobs
        let active_jobs = self.executor.active_job_count() as f64;
        let max_concurrency = self.concurrency_controller.max_concurrency().await as f64;
        
        (active_jobs / max_concurrency).min(1.0)
    }

    /// Get current queue depth
    async fn get_queue_depth(&self) -> usize {
        // This would be provided by the storage backend
        // For now, return a placeholder
        0
    }

    /// Get performance insights and recommendations
    pub async fn get_performance_insights(&self) -> PerformanceInsights {
        self.performance_optimizer.get_insights().await
    }

    /// Get current adaptive metrics
    pub async fn get_adaptive_metrics(&self) -> AdaptiveMetrics {
        AdaptiveMetrics {
            current_concurrency: self.concurrency_controller.current_concurrency().await,
            system_load: self.get_system_load().await,
            backpressure_level: self.backpressure_detector.detect_backpressure().await,
            active_jobs: self.executor.active_job_count(),
            available_permits: self.executor.available_permits(),
        }
    }
}

/// Intelligent concurrency controller
pub struct ConcurrencyController {
    /// Current concurrency level
    current_concurrency: Arc<RwLock<usize>>,
    
    /// Maximum allowed concurrency
    max_concurrency: usize,
    
    /// Minimum allowed concurrency
    min_concurrency: usize,
    
    /// Concurrency adjustment history
    adjustment_history: Arc<RwLock<Vec<ConcurrencyAdjustment>>>,
    
    /// Configuration
    config: ConcurrencyConfig,
}

impl ConcurrencyController {
    pub fn new(initial_concurrency: usize) -> Self {
        Self {
            current_concurrency: Arc::new(RwLock::new(initial_concurrency)),
            max_concurrency: initial_concurrency * 4,
            min_concurrency: 1,
            adjustment_history: Arc::new(RwLock::new(Vec::new())),
            config: ConcurrencyConfig::default(),
        }
    }

    pub fn with_config(config: ConcurrencyConfig) -> Self {
        Self {
            current_concurrency: Arc::new(RwLock::new(config.initial_concurrency)),
            max_concurrency: config.max_concurrency,
            min_concurrency: config.min_concurrency,
            adjustment_history: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    /// Calculate optimal concurrency based on system metrics
    pub async fn calculate_optimal_concurrency(
        &self,
        system_load: f64,
        queue_depth: usize,
        current_concurrency: usize,
    ) -> usize {
        let target_load = self.config.target_cpu_utilization;
        
        // Calculate adjustment based on system load
        let load_adjustment = if system_load > target_load {
            // System is overloaded, reduce concurrency
            -((system_load - target_load) * current_concurrency as f64) as i32
        } else {
            // System has capacity, potentially increase concurrency
            let capacity = target_load - system_load;
            (capacity * current_concurrency as f64 * 0.5) as i32
        };
        
        // Calculate adjustment based on queue depth
        let queue_adjustment = if queue_depth > current_concurrency * 2 {
            // High queue depth, increase concurrency
            (queue_depth / current_concurrency).min(current_concurrency / 2) as i32
        } else {
            0
        };
        
        // Combine adjustments with dampening
        let total_adjustment = (load_adjustment + queue_adjustment) / 2;
        
        let new_concurrency = (current_concurrency as i32 + total_adjustment)
            .max(self.min_concurrency as i32)
            .min(self.max_concurrency as i32) as usize;
        
        debug!(
            "Concurrency calculation: load={:.2}, queue_depth={}, current={}, new={}",
            system_load, queue_depth, current_concurrency, new_concurrency
        );
        
        new_concurrency
    }

    pub async fn current_concurrency(&self) -> usize {
        *self.current_concurrency.read().await
    }

    pub async fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    pub async fn set_concurrency(&self, new_concurrency: usize) -> QueueResult<()> {
        let mut current = self.current_concurrency.write().await;
        let old_concurrency = *current;
        *current = new_concurrency;
        
        // Record adjustment
        let adjustment = ConcurrencyAdjustment {
            timestamp: Instant::now(),
            old_value: old_concurrency,
            new_value: new_concurrency,
            reason: "adaptive_scaling".to_string(),
        };
        
        self.adjustment_history.write().await.push(adjustment);
        
        Ok(())
    }
}

/// Backpressure detection system
pub struct BackpressureDetector {
    /// Response time history
    response_times: Arc<RwLock<Vec<Duration>>>,
    
    /// Error rate history
    error_rates: Arc<RwLock<Vec<f64>>>,
    
    /// Configuration
    config: BackpressureConfig,
}

impl BackpressureDetector {
    pub fn new() -> Self {
        Self {
            response_times: Arc::new(RwLock::new(Vec::new())),
            error_rates: Arc::new(RwLock::new(Vec::new())),
            config: BackpressureConfig::default(),
        }
    }

    pub fn with_config(config: BackpressureConfig) -> Self {
        Self {
            response_times: Arc::new(RwLock::new(Vec::new())),
            error_rates: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    /// Detect current backpressure level (0.0 to 1.0)
    pub async fn detect_backpressure(&self) -> f64 {
        let response_time_pressure = self.calculate_response_time_pressure().await;
        let error_rate_pressure = self.calculate_error_rate_pressure().await;
        
        // Combine pressures with weighted average
        (response_time_pressure * 0.7 + error_rate_pressure * 0.3).min(1.0)
    }

    async fn calculate_response_time_pressure(&self) -> f64 {
        let response_times = self.response_times.read().await;
        
        if response_times.len() < 10 {
            return 0.0;
        }
        
        let recent_times: Vec<_> = response_times.iter().rev().take(10).collect();
        let avg_recent = recent_times.iter().map(|d| d.as_millis() as f64).sum::<f64>() / recent_times.len() as f64;
        
        let baseline_times: Vec<_> = response_times.iter().take(50).collect();
        let avg_baseline = baseline_times.iter().map(|d| d.as_millis() as f64).sum::<f64>() / baseline_times.len() as f64;
        
        if avg_baseline == 0.0 {
            return 0.0;
        }
        
        ((avg_recent / avg_baseline) - 1.0).max(0.0).min(1.0)
    }

    async fn calculate_error_rate_pressure(&self) -> f64 {
        let error_rates = self.error_rates.read().await;
        
        if error_rates.len() < 5 {
            return 0.0;
        }
        
        let recent_error_rate = error_rates.iter().rev().take(5).sum::<f64>() / 5.0;
        
        (recent_error_rate * 10.0).min(1.0)
    }

    pub async fn record_response_time(&self, duration: Duration) {
        let mut response_times = self.response_times.write().await;
        response_times.push(duration);
        
        // Keep only recent history
        if response_times.len() > 1000 {
            response_times.drain(0..500);
        }
    }

    pub async fn record_error_rate(&self, error_rate: f64) {
        let mut error_rates = self.error_rates.write().await;
        error_rates.push(error_rate);
        
        // Keep only recent history
        if error_rates.len() > 100 {
            error_rates.drain(0..50);
        }
    }
}

/// Performance optimization engine
pub struct PerformanceOptimizer {
    /// Job performance profiles
    job_profiles: Arc<DashMap<String, JobProfile>>,
    
    /// Optimization recommendations
    recommendations: Arc<RwLock<Vec<OptimizationRecommendation>>>,
    
    /// Configuration
    config: OptimizationConfig,
}

impl PerformanceOptimizer {
    pub fn new() -> Self {
        Self {
            job_profiles: Arc::new(DashMap::new()),
            recommendations: Arc::new(RwLock::new(Vec::new())),
            config: OptimizationConfig::default(),
        }
    }

    pub fn with_config(config: OptimizationConfig) -> Self {
        Self {
            job_profiles: Arc::new(DashMap::new()),
            recommendations: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    pub async fn record_execution(&self, job_type: &str, duration: Duration, success: bool) {
        let mut profile = self.job_profiles
            .entry(job_type.to_string())
            .or_insert_with(JobProfile::new);
        
        // Update profile would be called here
        // profile.update(duration, 0.0, 0);
        
        // Generate recommendations if needed
        if profile.execution_count % 100 == 0 {
            self.generate_recommendations(job_type, &profile).await;
        }
    }

    async fn generate_recommendations(&self, job_type: &str, profile: &JobProfile) {
        let mut recommendations = self.recommendations.write().await;
        
        // Analyze performance patterns and generate recommendations
        if profile.avg_duration > Duration::from_secs(30) {
            recommendations.push(OptimizationRecommendation {
                job_type: job_type.to_string(),
                recommendation_type: RecommendationType::ReduceTimeout,
                description: "Consider optimizing long-running job or increasing timeout".to_string(),
                impact: Impact::Medium,
                timestamp: Instant::now(),
            });
        }
        
        if profile.success_rate < 0.9 {
            recommendations.push(OptimizationRecommendation {
                job_type: job_type.to_string(),
                recommendation_type: RecommendationType::ImproveErrorHandling,
                description: "High failure rate detected, review error handling".to_string(),
                impact: Impact::High,
                timestamp: Instant::now(),
            });
        }
    }

    pub async fn get_insights(&self) -> PerformanceInsights {
        let recommendations = self.recommendations.read().await.clone();
        
        PerformanceInsights {
            recommendations,
            job_count: self.job_profiles.len(),
            optimization_score: self.calculate_optimization_score().await,
        }
    }

    async fn calculate_optimization_score(&self) -> f64 {
        // Calculate overall system optimization score
        let total_jobs = self.job_profiles.len() as f64;
        if total_jobs == 0.0 {
            return 1.0;
        }
        
        let avg_success_rate = self.job_profiles
            .iter()
            .map(|entry| entry.value().success_rate)
            .sum::<f64>() / total_jobs;
        
        avg_success_rate
    }
}

// Configuration structures
#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    pub initial_concurrency: usize,
    pub concurrency_config: ConcurrencyConfig,
    pub backpressure_config: BackpressureConfig,
    pub optimization_config: OptimizationConfig,
}

#[derive(Debug, Clone)]
pub struct ConcurrencyConfig {
    pub initial_concurrency: usize,
    pub min_concurrency: usize,
    pub max_concurrency: usize,
    pub target_cpu_utilization: f64,
    pub adjustment_interval: Duration,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            initial_concurrency: num_cpus::get().max(4),
            min_concurrency: 1,
            max_concurrency: num_cpus::get() * 4,
            target_cpu_utilization: 0.8,
            adjustment_interval: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    pub response_time_threshold: Duration,
    pub error_rate_threshold: f64,
    pub detection_window: Duration,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            response_time_threshold: Duration::from_secs(5),
            error_rate_threshold: 0.1,
            detection_window: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    pub analysis_interval: Duration,
    pub recommendation_threshold: usize,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            analysis_interval: Duration::from_secs(300),
            recommendation_threshold: 100,
        }
    }
}

// Data structures
#[derive(Debug, Clone)]
struct ConcurrencyAdjustment {
    timestamp: Instant,
    old_value: usize,
    new_value: usize,
    reason: String,
}

#[derive(Debug, Clone)]
pub struct OptimizationRecommendation {
    pub job_type: String,
    pub recommendation_type: RecommendationType,
    pub description: String,
    pub impact: Impact,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub enum RecommendationType {
    ReduceTimeout,
    ImproveErrorHandling,
    OptimizeResources,
    AdjustConcurrency,
}

#[derive(Debug, Clone)]
pub enum Impact {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct PerformanceInsights {
    pub recommendations: Vec<OptimizationRecommendation>,
    pub job_count: usize,
    pub optimization_score: f64,
}

#[derive(Debug, Clone)]
pub struct AdaptiveMetrics {
    pub current_concurrency: usize,
    pub system_load: f64,
    pub backpressure_level: f64,
    pub active_jobs: usize,
    pub available_permits: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_concurrency_controller() {
        let controller = ConcurrencyController::new(4);
        
        assert_eq!(controller.current_concurrency().await, 4);
        
        let optimal = controller.calculate_optimal_concurrency(0.9, 10, 4).await;
        assert!(optimal <= 4); // Should reduce due to high load
        
        controller.set_concurrency(optimal).await.unwrap();
        assert_eq!(controller.current_concurrency().await, optimal);
    }

    #[tokio::test]
    async fn test_backpressure_detector() {
        let detector = BackpressureDetector::new();
        
        // Initially no backpressure
        let pressure = detector.detect_backpressure().await;
        assert_eq!(pressure, 0.0);
        
        // Record some response times
        for _ in 0..20 {
            detector.record_response_time(Duration::from_millis(100)).await;
        }
        
        // Record higher response times
        for _ in 0..10 {
            detector.record_response_time(Duration::from_millis(500)).await;
        }
        
        let pressure = detector.detect_backpressure().await;
        assert!(pressure > 0.0);
    }

    #[tokio::test]
    async fn test_performance_optimizer() {
        let optimizer = PerformanceOptimizer::new();
        
        // Record some executions
        for _ in 0..150 {
            optimizer.record_execution("test_job", Duration::from_millis(100), true).await;
        }
        
        let insights = optimizer.get_insights().await;
        assert_eq!(insights.job_count, 1);
        assert!(insights.optimization_score > 0.0);
    }
}
