//! Stable tenant routing over caller-provisioned backends, without choosing a
//! database or broker. Keep shard count/order fixed for the life of stored jobs.
use super::*;
use std::sync::Arc;

pub struct ShardedBackend<B: ?Sized> {
    shards: Vec<Arc<B>>,
}
impl<B: ?Sized> ShardedBackend<B> {
    /// Shard order and count are persistent topology, not a live tuning knob.
    /// Changing either requires an offline tenant/data migration first.
    pub fn new(shards: Vec<Arc<B>>) -> QueueResult<Self> {
        if shards.is_empty() {
            return Err(QueueError::InvalidConfig(
                "at least one shard is required".into(),
            ));
        }
        Ok(Self { shards })
    }
    /// FNV-1a 64 over UTF-8, modulo the fixed shard count. Deliberately independent
    /// of Rust's randomized/default hash implementation and process restarts.
    pub fn shard_index(&self, tenant: &str) -> usize {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in tenant.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        (hash % self.shards.len() as u64) as usize
    }
    pub fn backend_for(&self, tenant: &str) -> &Arc<B> {
        &self.shards[self.shard_index(tenant)]
    }
}
#[async_trait]
impl<B: QueueBackend + ?Sized> QueueBackend for ShardedBackend<B> {
    async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
        self.backend_for(&ctx.tenant_id).enqueue(ctx, message).await
    }
    async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
        self.backend_for(&ctx.tenant_id).dequeue(ctx, queues).await
    }
    async fn ack_complete(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        result: Option<String>,
    ) -> QueueResult<()> {
        self.backend_for(&ctx.tenant_id)
            .ack_complete(ctx, id, token, result)
            .await
    }
    async fn ack_fail(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        error: String,
        retry: Option<DateTime<Utc>>,
    ) -> QueueResult<()> {
        self.backend_for(&ctx.tenant_id)
            .ack_fail(ctx, id, token, error, retry)
            .await
    }
    async fn heartbeat_extend(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        duration: Duration,
    ) -> QueueResult<()> {
        self.backend_for(&ctx.tenant_id)
            .heartbeat_extend(ctx, id, token, duration)
            .await
    }
    async fn cancel(&self, ctx: QueueCtx, id: JobId) -> QueueResult<bool> {
        self.backend_for(&ctx.tenant_id).cancel(ctx, id).await
    }
    async fn get_status(&self, ctx: QueueCtx, id: JobId) -> QueueResult<JobStatus> {
        self.backend_for(&ctx.tenant_id).get_status(ctx, id).await
    }
    async fn get_record(&self, ctx: QueueCtx, id: JobId) -> QueueResult<JobRecord> {
        self.backend_for(&ctx.tenant_id).get_record(ctx, id).await
    }
    async fn get_snapshot(&self, ctx: QueueCtx, id: JobId) -> QueueResult<crate::JobSnapshot> {
        self.backend_for(&ctx.tenant_id).get_snapshot(ctx, id).await
    }
    async fn get_snapshots(
        &self,
        ctx: QueueCtx,
        ids: &[JobId],
    ) -> QueueResult<Vec<crate::JobSnapshot>> {
        self.backend_for(&ctx.tenant_id)
            .get_snapshots(ctx, ids)
            .await
    }
    fn event_stream(&self, ctx: QueueCtx) -> BoxStream<JobEvent> {
        self.backend_for(&ctx.tenant_id).event_stream(ctx)
    }
    async fn reclaim_expired_leases(&self) -> QueueResult<Vec<ReapOutcome>> {
        let mut result = Vec::new();
        for shard in &self.shards {
            result.extend(shard.reclaim_expired_leases().await?);
        }
        Ok(result)
    }
    fn capabilities(&self) -> QueueCapabilities {
        let mut result = self.shards[0].capabilities();
        for shard in &self.shards[1..] {
            let c = shard.capabilities();
            result.delayed &= c.delayed;
            result.scheduled_at &= c.scheduled_at;
            result.cancel &= c.cancel;
            result.lease_extend &= c.lease_extend;
            result.priority &= c.priority;
            result.idempotency &= c.idempotency;
            result.dead_letter_queue &= c.dead_letter_queue;
        }
        result
    }
}
impl<B: broker::JobLedger + ?Sized> broker::JobLedger for ShardedBackend<B> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::memory::MemoryBackend;
    #[tokio::test]
    async fn routers_share_ownership_and_isolation_across_independent_shards() {
        let shards: Vec<_> = (0..16).map(|_| Arc::new(MemoryBackend::new())).collect();
        let a = ShardedBackend::new(shards.clone()).unwrap();
        let b = ShardedBackend::new(shards).unwrap();
        assert_eq!(a.shard_index("hello"), 11); // published FNV-1a vector a430d84680aabd0b
        let mut used = std::collections::HashSet::new();
        for n in 0..100 {
            let ctx = QueueCtx::new(format!("tenant-{n}"));
            used.insert(a.shard_index(&ctx.tenant_id));
            let id = a
                .enqueue(
                    ctx.clone(),
                    JobMessage::new("test", vec![1, 2, 3], "bytes", "q"),
                )
                .await
                .unwrap();
            assert!(b
                .get_record(QueueCtx::new("foreign"), id.clone())
                .await
                .is_err());
            let job = b.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
            assert_eq!(job.record.job_id, id);
            a.ack_complete(ctx.clone(), id.clone(), job.lease_token, None)
                .await
                .unwrap();
            assert!(b.get_status(ctx, id).await.unwrap().is_terminal());
        }
        assert_eq!(used.len(), 16);
    }
}
