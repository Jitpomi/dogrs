//! Durable NATS queue using JetStream KV compare-and-swap, not Core NATS delivery.
use super::durable::DurableBackend;
use crate::{QueueError, QueueResult};
use async_nats::jetstream::{self, kv, stream};

#[derive(Clone)]
pub struct NatsConfig {
    pub url: String,
    /// Dedicated KV bucket name (letters, numbers, underscores and hyphens).
    /// This field was a Core NATS subject before the durable queue release.
    pub subject: String,
}
pub struct NatsStore {
    pub(super) enqueue_slots: tokio::sync::Semaphore,
    pub(super) bucket: kv::Store,
    pub(super) max_state_bytes: usize,
    pub(super) index: tokio::sync::OnceCell<std::sync::Arc<super::nats_records::Index>>,
    pub(super) checked_tenants: dashmap::DashMap<String, std::sync::Arc<tokio::sync::OnceCell<()>>>,
}
pub type NatsBackend = DurableBackend<NatsStore>;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}

impl NatsBackend {
    /// Bound concurrent submissions before they reach JetStream. Claims and
    /// lease updates do not take producer permits. Defaults to 16 per backend.
    pub fn with_enqueue_concurrency(mut self, limit: usize) -> QueueResult<Self> {
        if !(1..=4096).contains(&limit) {
            return Err(QueueError::InvalidConfig(
                "NATS enqueue concurrency must be within 1..=4096".into(),
            ));
        }
        self.store.enqueue_slots = tokio::sync::Semaphore::new(limit);
        Ok(self)
    }
    /// Local/development convenience: creates a file-backed bucket with one replica.
    /// For clustered production use, provision the bucket and call `from_store`.
    pub async fn new(config: NatsConfig) -> QueueResult<Self> {
        let client = async_nats::connect(config.url.split(',').collect::<Vec<_>>())
            .await
            .map_err(error)?;
        let max_payload = client.server_info().max_payload;
        let js = jetstream::new(client);
        let bucket = match js.get_key_value(&config.subject).await {
            Ok(bucket) => bucket,
            Err(_) => match js
                .create_key_value(kv::Config {
                    bucket: config.subject.clone(),
                    history: 1,
                    storage: stream::StorageType::File,
                    num_replicas: 1,
                    ..Default::default()
                })
                .await
            {
                Ok(bucket) => bucket,
                // Another process may have created the bucket concurrently.
                Err(_) => js.get_key_value(&config.subject).await.map_err(error)?,
            },
        };
        let mut config = bucket.stream.cached_info().config.clone();
        // Leader reads avoid observing stale follower state. CAS protects every write.
        if config.allow_direct {
            config.allow_direct = false;
            js.update_stream(config).await.map_err(error)?;
        }
        Self::from_store_with_max_payload(
            js.get_key_value(&bucket.name).await.map_err(error)?,
            max_payload,
        )
    }
    pub async fn new_async(config: NatsConfig) -> QueueResult<Self> {
        Self::new(config).await
    }

    /// Require a replicated deployment without choosing a hosting vendor. The
    /// caller must separately verify server fsync policy and failure-domain placement.
    pub fn from_replicated_store(
        bucket: kv::Store,
        max_payload: usize,
        min_replicas: usize,
    ) -> QueueResult<Self> {
        if min_replicas < 3 || bucket.stream.cached_info().config.num_replicas < min_replicas {
            return Err(QueueError::InvalidConfig("Replicated JetStream ledger requires at least the requested three or more replicas".into()));
        }
        Self::from_store_with_max_payload(bucket, max_payload)
    }

    /// Use a caller-authenticated, TLS-connected JetStream bucket. Provision it with
    /// file storage, no expiration, no eviction of old keys, and leader-only reads.
    pub fn from_store(bucket: kv::Store) -> QueueResult<Self> {
        Self::from_store_with_max_payload(bucket, 1024 * 1024)
    }

    /// Supply the smaller of the server and account payload limits. Hosted account
    /// limits may be lower than Client::server_info().max_payload. This reserves
    /// framing space and future completion metadata before admitting a job.
    pub fn from_store_with_max_payload(bucket: kv::Store, max_payload: usize) -> QueueResult<Self> {
        let config = &bucket.stream.cached_info().config;
        if config.storage != stream::StorageType::File
            || !config.max_age.is_zero()
            || config.allow_direct
            || config.discard != stream::DiscardPolicy::New
        {
            return Err(QueueError::InvalidConfig(
                "NATS queue requires file storage, max_age=0, allow_direct=false and discard=new"
                    .into(),
            ));
        }
        let max_state_bytes = state_budget(max_payload, config.max_message_size)?;
        Ok(Self {
            store: NatsStore {
                enqueue_slots: tokio::sync::Semaphore::new(16),
                bucket,
                max_state_bytes,
                index: Default::default(),
                checked_tenants: Default::default(),
            },
            lease_duration: std::time::Duration::from_secs(300),
        })
    }
}

/// Compatibility import path.
#[allow(clippy::module_inception)] // Preserve the pre-0.2 import path.
pub mod nats {
    pub use super::{NatsBackend, NatsConfig, NatsStore};
}

// Headers and the encoded tenant key also consume protocol payload space.
fn state_budget(max_payload: usize, stream_limit: i32) -> QueueResult<usize> {
    let limit = if stream_limit > 0 {
        max_payload.min(stream_limit as usize)
    } else {
        max_payload
    };
    if limit < 16_384 {
        return Err(QueueError::InvalidConfig(
            "NATS payload limit must be at least 16 KiB".into(),
        ));
    }
    Ok(900_000.min(limit - 4096))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_uses_account_and_stream_limits() {
        assert_eq!(state_budget(512 * 1024, -1).unwrap(), 520_192);
        assert_eq!(state_budget(8 * 1024 * 1024, 128 * 1024).unwrap(), 126_976);
        assert_eq!(state_budget(1024 * 1024, -1).unwrap(), 900_000);
        assert!(state_budget(4096, -1).is_err());
    }
}
