//! Generic tenant worker discovery, recovery and graceful pool ownership.
use crate::{QueueError, QueueResult, WorkerHandle};
use async_trait::async_trait;
use futures::FutureExt;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
#[async_trait]
pub trait TenantWorkers: Send + Sync + 'static {
    /// Sorted, bounded page strictly after the supplied tenant cursor.
    async fn page(&self, after: Option<&str>) -> QueueResult<Vec<String>>;
    /// Return ownership of a fully initialized tenant pool.
    async fn start(&self, tenant: &str) -> QueueResult<WorkerHandle>;
}
#[must_use = "Await shutdown before retiring a database"]
pub struct SupervisorHandle {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<QueueResult<()>>>,
}
impl SupervisorHandle {
    pub async fn shutdown(mut self) -> QueueResult<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.await
                .map_err(|_| QueueError::Internal("Tenant supervisor failed".into()))??;
        }
        Ok(())
    }
}
impl Drop for SupervisorHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
pub fn start(source: Arc<dyn TenantWorkers>) -> SupervisorHandle {
    let (stop, mut stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut pools: BTreeMap<String, WorkerHandle> = BTreeMap::new();
        let supervision = std::panic::AssertUnwindSafe(async {
            let mut cursor = None;
            let mut clock = tokio::time::interval(Duration::from_secs(5));
            clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {biased;_=&mut stopped=>break,_=clock.tick()=>{}}
                let tenants = match source.page(cursor.as_deref()).await {
                    Ok(rows) => rows,
                    Err(_) => {
                        tracing::warn!("Queue tenant discovery deferred");
                        continue;
                    }
                };
                if tenants.is_empty() {
                    cursor = None;
                    continue;
                }
                for tenant in tenants {
                    if cursor.as_ref().is_some_and(|last: &String| last >= &tenant) {
                        return Err(QueueError::InvalidConfig(
                            "Tenant pages must be strictly ordered".into(),
                        ));
                    }
                    cursor = Some(tenant.clone());
                    if pools.get(&tenant).is_some_and(|pool| !pool.is_finished()) {
                        continue;
                    }
                    if let Some(pool) = pools.remove(&tenant) {
                        if pool.shutdown().await.is_err() {
                            tracing::error!(
                                tenant,
                                "Queue pool failed; recovering durable deliveries"
                            );
                        }
                    }
                    match source.start(&tenant).await {
                        Ok(pool) => {
                            pools.insert(tenant, pool);
                        }
                        Err(_) => tracing::warn!(tenant, "Queue pool startup deferred"),
                    }
                }
            }
            Ok(())
        })
        .catch_unwind()
        .await;
        // Keep ownership outside the unwind boundary: even a source panic drains
        // existing effects before shutdown reports failure.
        let results =
            futures::future::join_all(pools.into_values().map(WorkerHandle::shutdown)).await;
        let errors: Vec<_> = results
            .into_iter()
            .filter_map(Result::err)
            .map(|e| e.to_string())
            .collect();
        supervision.map_err(|_| {
            QueueError::Internal("Tenant discovery panicked; pools drained".into())
        })??;
        if !errors.is_empty() {
            return Err(QueueError::Internal(format!(
                "Queue pool drain failed: {}",
                errors.join("; ")
            )));
        }
        Ok(())
    });
    SupervisorHandle {
        stop: Some(stop),
        task: Some(task),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::QueueBackend;
    #[derive(serde::Serialize, serde::Deserialize)]
    struct DrainJob;
    #[derive(Clone)]
    struct Context {
        started: Arc<tokio::sync::Notify>,
        finish: Arc<tokio::sync::Notify>,
    }
    #[async_trait]
    impl crate::Job for DrainJob {
        type Context = Context;
        type Result = ();
        const JOB_TYPE: &'static str = "supervisor.drain";
        async fn execute(&self, context: Context) -> Result<(), crate::JobError> {
            context.started.notify_one();
            context.finish.notified().await;
            Ok(())
        }
    }
    struct Source {
        adapter: crate::QueueAdapter<crate::backend::memory::MemoryBackend>,
        context: Context,
    }
    #[async_trait]
    impl TenantWorkers for Source {
        async fn page(&self, after: Option<&str>) -> QueueResult<Vec<String>> {
            Ok(if after.is_none() {
                vec!["tenant".into()]
            } else {
                vec![]
            })
        }
        async fn start(&self, tenant: &str) -> QueueResult<WorkerHandle> {
            self.adapter
                .start_workers(
                    crate::QueueCtx::new(tenant),
                    self.context.clone(),
                    vec!["supervisor.drain".into()],
                )
                .await
        }
    }
    #[tokio::test]
    async fn supervisor_shutdown_waits_for_handlers_and_acknowledgements() {
        let adapter = crate::QueueAdapter::with_config(
            crate::backend::memory::MemoryBackend::new(),
            crate::QueueConfig {
                max_workers: 1,
                ..Default::default()
            },
        );
        adapter.register_job::<DrainJob>().await.unwrap();
        let ctx = crate::QueueCtx::new("tenant");
        let id = adapter.enqueue(ctx.clone(), DrainJob).await.unwrap();
        let context = Context {
            started: Arc::new(tokio::sync::Notify::new()),
            finish: Arc::new(tokio::sync::Notify::new()),
        };
        let source = Arc::new(Source {
            adapter,
            context: context.clone(),
        });
        let handle = start(source.clone());
        tokio::time::timeout(Duration::from_secs(2), context.started.notified())
            .await
            .unwrap();
        let stopping = tokio::spawn(handle.shutdown());
        tokio::task::yield_now().await;
        assert!(!stopping.is_finished());
        context.finish.notify_one();
        tokio::time::timeout(Duration::from_secs(2), stopping)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            source.adapter.backend().get_status(ctx, id).await.unwrap(),
            crate::JobStatus::Completed { .. }
        ));
    }
    struct Panics;
    #[async_trait]
    impl TenantWorkers for Panics {
        async fn page(&self, _: Option<&str>) -> QueueResult<Vec<String>> {
            panic!("synthetic discovery failure")
        }
        async fn start(&self, _: &str) -> QueueResult<WorkerHandle> {
            unreachable!()
        }
    }
    #[tokio::test]
    async fn discovery_failure_is_reported_instead_of_false_quiescence() {
        let worker = start(Arc::new(Panics));
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(worker.shutdown().await.is_err());
    }
}
