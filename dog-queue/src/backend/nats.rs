//! Durable NATS queue using JetStream KV compare-and-swap, not Core NATS delivery.
use super::durable::{DurableBackend, Operation, Outcome, StateStore, TenantState};
use crate::{QueueError, QueueResult};
use async_nats::jetstream::{self, kv, stream};
use async_trait::async_trait;
use futures::StreamExt;

#[derive(Clone)]
pub struct NatsConfig {
    pub url: String,
    /// Dedicated KV bucket name (letters, numbers, underscores and hyphens).
    /// This field was a Core NATS subject before the durable queue release.
    pub subject: String,
}
pub struct NatsStore {
    bucket: kv::Store,
    max_state_bytes: usize,
}
pub type NatsBackend = DurableBackend<NatsStore>;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}

impl NatsBackend {
    /// Local/development convenience: creates a file-backed bucket with one replica.
    /// For clustered production use, provision the bucket and call `from_store`.
    pub async fn new(config: NatsConfig) -> QueueResult<Self> {
        let client = async_nats::connect(config.url).await.map_err(error)?;
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
                bucket,
                max_state_bytes,
            },
            lease_duration: std::time::Duration::from_secs(300),
        })
    }
}

#[async_trait]
impl StateStore for NatsStore {
    async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        let key = format!(
            "tenant_{}",
            tenant
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        for _ in 0..64 {
            let previous = self.bucket.entry(&key).await.map_err(error)?;
            let revision = previous.as_ref().map(|e| e.revision).unwrap_or(0);
            let mut state: TenantState = match previous {
                Some(entry) if entry.operation == kv::Operation::Put => {
                    serde_json::from_slice(&entry.value).map_err(error)?
                }
                _ => TenantState::default(),
            };
            let outcome = state.apply(tenant, op)?;
            if matches!(
                op,
                Operation::Get(_) | Operation::Snapshot(_) | Operation::Snapshots(_)
            ) {
                return Ok(outcome);
            }
            let value = serde_json::to_vec(&state).map_err(error)?;
            if matches!(op, Operation::Enqueue(_))
                && value.len().saturating_add(state.reserved_bytes()) > self.max_state_bytes
            {
                return Err(QueueError::InvalidConfig("JetStream tenant capacity reached; purge terminal history or use a larger-capacity ledger".into()));
            }
            match self.bucket.update(&key, value.into(), revision).await {
                Ok(_) => return Ok(outcome),
                Err(err) if err.kind() == kv::UpdateErrorKind::WrongLastRevision => {
                    tokio::task::yield_now().await
                }
                Err(err) => return Err(error(err)),
            }
        }
        Err(QueueError::Internal(
            "NATS queue contention: retry operation".into(),
        ))
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        let mut keys = self.bucket.keys().await.map_err(error)?;
        let mut tenants = Vec::new();
        while let Some(key) = keys.next().await {
            let key = key.map_err(error)?;
            let Some(encoded) = key.strip_prefix("tenant_") else {
                continue;
            };
            if encoded.len() % 2 != 0 || !encoded.is_ascii() {
                return Err(error("Invalid NATS tenant key"));
            }
            let bytes = (0..encoded.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16))
                .collect::<Result<Vec<_>, _>>()
                .map_err(error)?;
            tenants.push(String::from_utf8(bytes).map_err(error)?);
        }
        Ok(tenants)
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
