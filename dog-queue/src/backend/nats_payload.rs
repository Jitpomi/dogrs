//! Experimental immutable bundles. References, membership index and bytes commit
//! together. Per-job ownership stays in the independent metadata store.
use super::{nats::NatsStore, nats_batch::Write};
use crate::{QueueError, QueueResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tokio::sync::OnceCell;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
#[derive(Serialize, Deserialize)]
struct PayloadRef {
    key: String,
    bundle: String,
    offset: usize,
    len: usize,
    digest: [u8; 32],
}
#[derive(Serialize, Deserialize)]
struct BundleIndex {
    keys: Vec<String>,
}
#[derive(Default)]
pub(super) struct PayloadCache {
    entries: HashMap<String, Arc<OnceCell<Vec<u8>>>>,
    order: VecDeque<String>,
}
impl PayloadCache {
    fn cell(&mut self, key: &str) -> Arc<OnceCell<Vec<u8>>> {
        if let Some(cell) = self.entries.get(key).cloned() {
            self.order.retain(|old| old != key);
            self.order.push_back(key.into());
            return cell;
        }
        while self.entries.len() >= 16 {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
        let cell = Arc::new(OnceCell::new());
        self.entries.insert(key.into(), cell.clone());
        self.order.push_back(key.into());
        cell
    }
    fn remove(&mut self, key: &str) {
        self.entries.remove(key);
        self.order.retain(|old| old != key);
    }
}
pub(super) fn pack(writes: &[&Write], limit: usize) -> QueueResult<Vec<Write>> {
    if writes.is_empty()
        || writes.len() > 126
        || writes
            .iter()
            .any(|w| !(w.key.starts_with("a.") || (w.key.starts_with("p.") && w.revision == 0)))
    {
        return Err(error("invalid immutable payload bundle"));
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let bundle = format!("b.{id}");
    let mut body = Vec::new();
    let mut packed = Vec::with_capacity(writes.len() + 2);
    for write in writes {
        if write.key.starts_with("a.") {
            packed.push(Write {
                key: write.key.clone(),
                value: write.value.clone(),
                revision: write.revision,
            });
            continue;
        }
        let reference = PayloadRef {
            key: write.key.clone(),
            bundle: bundle.clone(),
            offset: body.len(),
            len: write.value.len(),
            digest: Sha256::digest(&write.value).into(),
        };
        body.extend_from_slice(&write.value);
        packed.push(Write {
            key: write.key.clone(),
            value: serde_json::to_vec(&reference).map_err(error)?,
            revision: 0,
        });
    }
    packed.push(Write {
        key: format!("i.{id}"),
        value: serde_json::to_vec(&BundleIndex {
            keys: writes
                .iter()
                .filter(|w| w.key.starts_with("p."))
                .map(|w| w.key.clone())
                .collect(),
        })
        .map_err(error)?,
        revision: 0,
    });
    packed.push(Write {
        key: bundle,
        value: body,
        revision: 0,
    });
    if packed.iter().any(|w| w.value.len() > limit)
        || packed.iter().map(|w| w.value.len()).sum::<usize>() > 2 * 1024 * 1024
    {
        return Err(error("immutable payload bundle exceeds storage bounds"));
    }
    Ok(packed)
}
impl NatsStore {
    pub(super) async fn read_packed(&self, key: &str) -> QueueResult<Vec<u8>> {
        let value = self
            .payload_bucket
            .get(key)
            .await
            .map_err(error)?
            .ok_or_else(|| error("payload reference missing"))?;
        let reference: PayloadRef = serde_json::from_slice(&value).map_err(error)?;
        if reference.key != key
            || !reference.bundle.starts_with("b.")
            || reference.len > self.max_state_bytes
        {
            return Err(error("invalid payload reference"));
        }
        uuid::Uuid::parse_str(&reference.bundle[2..]).map_err(error)?;
        let cell = self
            .payload_cache
            .lock()
            .map_err(error)?
            .cell(&reference.bundle);
        let bytes = cell
            .get_or_try_init(|| async {
                self.payload_bucket
                    .get(&reference.bundle)
                    .await
                    .map_err(error)?
                    .map(|b| b.to_vec())
                    .ok_or_else(|| error("immutable payload bundle missing"))
            })
            .await?;
        let end = reference
            .offset
            .checked_add(reference.len)
            .ok_or_else(|| error("invalid payload range"))?;
        let payload = bytes
            .get(reference.offset..end)
            .ok_or_else(|| error("invalid payload range"))?;
        if <[u8; 32]>::from(Sha256::digest(payload)) != reference.digest {
            return Err(error("payload digest mismatch"));
        }
        Ok(payload.to_vec())
    }
    pub(super) async fn purge_packed(&self, key: &str) -> QueueResult<()> {
        let Some(value) = self.payload_bucket.get(key).await.map_err(error)? else {
            return Ok(());
        };
        let reference: PayloadRef = serde_json::from_slice(&value).map_err(error)?;
        if reference.key != key || !reference.bundle.starts_with("b.") {
            return Err(error("invalid payload reference"));
        }
        uuid::Uuid::parse_str(&reference.bundle[2..]).map_err(error)?;
        let index_key = format!("i.{}", &reference.bundle[2..]);
        let index = self
            .payload_bucket
            .get(&index_key)
            .await
            .map_err(error)?
            .ok_or_else(|| error("bundle index missing"))?;
        let index: BundleIndex = serde_json::from_slice(&index).map_err(error)?;
        if !index.keys.iter().any(|k| k == key) {
            return Err(error("reference absent from bundle index"));
        }
        self.payload_bucket.purge(key).await.map_err(error)?;
        for other in index.keys {
            if self
                .payload_bucket
                .get(other)
                .await
                .map_err(error)?
                .is_some()
            {
                return Ok(());
            }
        }
        // References never reappear: immutable writes use expected revision zero.
        self.payload_bucket
            .purge(&reference.bundle)
            .await
            .map_err(error)?;
        self.payload_bucket.purge(index_key).await.map_err(error)?;
        self.payload_cache
            .lock()
            .map_err(error)?
            .remove(&reference.bundle);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundle_references_cover_exact_bytes_and_cache_is_bounded() {
        let writes: Vec<_> = (0..13)
            .map(|i| Write {
                key: format!("p.tenant.{i}"),
                value: vec![i as u8; 65536],
                revision: 0,
            })
            .collect();
        let packed = pack(&writes.iter().collect::<Vec<_>>(), 900000).unwrap();
        assert_eq!(packed.len(), 15);
        let body = &packed.last().unwrap().value;
        for (i, write) in packed.iter().take(13).enumerate() {
            let reference: PayloadRef = serde_json::from_slice(&write.value).unwrap();
            assert_eq!(reference.key, writes[i].key);
            assert_eq!(
                &body[reference.offset..reference.offset + reference.len],
                &writes[i].value
            );
            assert_eq!(
                reference.digest,
                <[u8; 32]>::from(Sha256::digest(&writes[i].value))
            );
        }
        let mut cache = PayloadCache::default();
        let a = cache.cell("one");
        assert!(Arc::ptr_eq(&a, &cache.cell("one")));
        for i in 0..1000 {
            cache.cell(&i.to_string());
        }
        assert_eq!(cache.entries.len(), 16);
        assert_eq!(cache.order.len(), 16);
        assert!(!cache.entries.contains_key("one"));
    }
    #[tokio::test]
    #[ignore = "requires disposable NATS 2.12+ with atomic publishing"]
    async fn packed_payloads_share_storage_without_sharing_tenant_lifetimes() {
        use crate::{backend::nats::NatsBackend, JobMessage, QueueBackend, QueueCtx};
        use async_nats::jetstream::{self, kv, stream};
        use futures::TryStreamExt;
        let js = jetstream::new(
            async_nats::connect(std::env::var("DOGRS_NATS_URL").unwrap())
                .await
                .unwrap(),
        );
        for combined in [false, true] {
            let name = format!("packed_{}", uuid::Uuid::new_v4().simple());
            let payload_name = format!("{name}_payload");
            for name in [&name, &payload_name] {
                let bucket = js
                    .create_key_value(kv::Config {
                        bucket: name.clone(),
                        storage: stream::StorageType::File,
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                let mut config = bucket.stream.cached_info().config.clone();
                config.allow_direct = false;
                config.allow_atomic_publish = true;
                js.update_stream(config).await.unwrap();
            }
            async fn open(
                js: jetstream::Context,
                name: &str,
                payload_name: &str,
                combined: bool,
            ) -> NatsBackend {
                if combined {
                    NatsBackend::from_context_with_payload_packing(js, name, 1024 * 1024)
                        .await
                        .unwrap()
                } else {
                    NatsBackend::from_context_with_packed_payload_bucket(
                        js,
                        name,
                        payload_name,
                        1024 * 1024,
                    )
                    .await
                    .unwrap()
                }
            }
            let backend = Arc::new(open(js.clone(), &name, &payload_name, combined).await);
            let mut producers = tokio::task::JoinSet::new();
            for i in 0..128u8 {
                let backend = backend.clone();
                producers.spawn(async move {
                    let tenant = if i % 2 == 0 { "a" } else { "b" };
                    let id = backend
                        .enqueue(
                            QueueCtx::new(tenant),
                            JobMessage::new("work", vec![i; 65536], "bytes", "q"),
                        )
                        .await
                        .unwrap();
                    (tenant, id, i)
                });
            }
            let mut jobs = Vec::new();
            while let Some(job) = producers.join_next().await {
                jobs.push(job.unwrap());
            }
            let keys = backend
                .store
                .payload_bucket
                .keys()
                .await
                .unwrap()
                .try_collect::<Vec<_>>()
                .await
                .unwrap();
            let bundles = keys.iter().filter(|k| k.starts_with("b.")).count();
            assert!(
                bundles < 128,
                "concurrent payloads should share immutable bundles"
            );
            let reader = open(js.clone(), &name, &payload_name, combined).await;
            for (tenant, id, i) in &jobs {
                let row = reader
                    .get_record(QueueCtx::new(*tenant), id.clone())
                    .await
                    .unwrap();
                assert_eq!(row.message.payload_bytes, vec![*i; 65536]);
                assert!(reader
                    .get_record(
                        QueueCtx::new(if *tenant == "a" { "b" } else { "a" }),
                        id.clone()
                    )
                    .await
                    .is_err());
                if *tenant == "a" {
                    reader
                        .cancel(QueueCtx::new(*tenant), id.clone())
                        .await
                        .unwrap();
                }
            }
            assert_eq!(
                reader
                    .purge_terminal_before(
                        QueueCtx::new("a"),
                        chrono::Utc::now() + chrono::Duration::seconds(1)
                    )
                    .await
                    .unwrap(),
                64
            );
            for (tenant, id, i) in &jobs {
                if *tenant == "b" {
                    assert_eq!(
                        reader
                            .get_record(QueueCtx::new("b"), id.clone())
                            .await
                            .unwrap()
                            .message
                            .payload_bytes,
                        vec![*i; 65536]
                    );
                    reader.cancel(QueueCtx::new("b"), id.clone()).await.unwrap();
                }
            }
            assert_eq!(
                reader
                    .purge_terminal_before(
                        QueueCtx::new("b"),
                        chrono::Utc::now() + chrono::Duration::seconds(1)
                    )
                    .await
                    .unwrap(),
                64
            );
            let keys = backend
                .store
                .payload_bucket
                .keys()
                .await
                .unwrap()
                .try_collect::<Vec<_>>()
                .await
                .unwrap();
            assert!(
                keys.iter().all(|k| !["p.", "b.", "i."]
                    .iter()
                    .any(|prefix| k.starts_with(prefix))),
                "last reference must release the bundle and its membership index"
            );
            js.delete_key_value(name).await.unwrap();
            js.delete_key_value(payload_name).await.unwrap();
        }
    }
}
