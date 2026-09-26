//! Redis queue with atomic compare-and-swap state transitions across processes.
//! Enable Redis persistence (AOF) and appropriate replication for durable storage.
use super::durable::{DurableBackend, Operation, Outcome, StateStore, TenantState};
use crate::{QueueError, QueueResult};
use async_trait::async_trait;
use redis::{aio::ConnectionManager, AsyncCommands};

#[derive(Clone)]
pub struct RedisConfig {
    pub connection_string: String,
}
pub struct RedisStore {
    manager: ConnectionManager,
}
pub type RedisBackend = DurableBackend<RedisStore>;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
const TENANTS: &str = "{dogrs-queue-v1}:tenants";

impl RedisBackend {
    pub async fn new(config: RedisConfig) -> QueueResult<Self> {
        let client = redis::Client::open(config.connection_string).map_err(error)?;
        let manager = client.get_connection_manager().await.map_err(error)?;
        Ok(Self {
            store: RedisStore { manager },
            lease_duration: std::time::Duration::from_secs(300),
        })
    }
}
#[async_trait]
impl StateStore for RedisStore {
    async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        let encoded: String = tenant
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let key = format!("{{dogrs-queue-v1}}:state:{encoded}");
        let mut conn = self.manager.clone();
        for _ in 0..64 {
            let previous: Option<String> = conn.get(&key).await.map_err(error)?;
            let mut state: TenantState = match &previous {
                Some(s) => serde_json::from_str(s).map_err(error)?,
                None => TenantState::default(),
            };
            let result = state.apply(tenant, op)?;
            if matches!(op, Operation::Get(_)) {
                return Ok(result);
            }
            let value = serde_json::to_string(&state).map_err(error)?;
            let changed: i32 = redis::Script::new("if (redis.call('GET',KEYS[1]) or '') ~= ARGV[1] then return 0 end; redis.call('SET',KEYS[1],ARGV[2]); redis.call('SADD',KEYS[2],ARGV[3]); return 1")
                .key(&key).key(TENANTS).arg(previous.as_deref().unwrap_or("")).arg(value).arg(tenant)
                .invoke_async(&mut conn).await.map_err(error)?;
            if changed == 1 {
                return Ok(result);
            }
            tokio::task::yield_now().await;
        }
        Err(QueueError::Internal(
            "Redis queue contention: retry operation".into(),
        ))
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        self.manager.clone().smembers(TENANTS).await.map_err(error)
    }
}
