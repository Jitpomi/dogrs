//! Transactional PostgreSQL queue. State is versioned and isolated per tenant.
//! Operations serialize per tenant; intended for modest queues, not bulk analytics.
use super::durable::{DurableBackend, Operation, Outcome, StateStore, TenantState};
use crate::{QueueError, QueueResult};
use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};

#[derive(Clone)]
pub struct PostgresConfig {
    pub connection_string: String,
}
type Connector = std::sync::Arc<
    dyn Fn() -> futures::future::BoxFuture<'static, QueueResult<Client>> + Send + Sync,
>;
pub struct PostgresStore {
    client: Mutex<Client>,
    connect: Connector,
}
impl PostgresStore {
    async fn client(&self) -> QueueResult<tokio::sync::MutexGuard<'_, Client>> {
        let mut client = self.client.lock().await;
        if client.is_closed() {
            *client = (self.connect)().await?;
        }
        Ok(client)
    }
}
pub type PostgresBackend = DurableBackend<PostgresStore>;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}

impl PostgresBackend {
    /// The connection must be local or secured by a trusted tunnel; this constructor
    /// uses NoTls. Existing legacy `jobs` tables are never read, dropped or rewritten.
    pub async fn new(config: PostgresConfig) -> QueueResult<Self> {
        Self::new_with_tls(config, NoTls).await
    }

    /// Connect with a caller-supplied, certificate-validating TLS connector.
    pub async fn new_with_tls<T>(config: PostgresConfig, tls: T) -> QueueResult<Self>
    where
        T: tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket>
            + Clone
            + Send
            + Sync
            + 'static,
        T::Stream: Send + 'static,
        T::TlsConnect: Send,
        <T::TlsConnect as tokio_postgres::tls::TlsConnect<tokio_postgres::Socket>>::Future: Send,
    {
        let connect: Connector = std::sync::Arc::new(move || {
            let connection_string = config.connection_string.clone();
            let tls = tls.clone();
            Box::pin(async move {
                let (client, connection) = tokio_postgres::connect(&connection_string, tls)
                    .await
                    .map_err(error)?;
                tokio::spawn(async move {
                    if let Err(err) = connection.await {
                        tracing::error!(%err, "PostgreSQL queue connection closed; next operation reconnects");
                    }
                });
                Ok(client)
            })
        });
        let client = connect().await?;
        client.batch_execute("CREATE TABLE IF NOT EXISTS dogrs_queue_state_v1 (tenant TEXT PRIMARY KEY, state JSONB NOT NULL)").await.map_err(error)?;
        Ok(Self {
            store: PostgresStore {
                client: Mutex::new(client),
                connect,
            },
            lease_duration: std::time::Duration::from_secs(300),
        })
    }
}
#[async_trait]
impl StateStore for PostgresStore {
    async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        let mut client = self.client().await?;
        let tx = client.transaction().await.map_err(error)?;
        let empty = serde_json::to_value(TenantState::default()).map_err(error)?;
        tx.execute(
            "INSERT INTO dogrs_queue_state_v1 (tenant,state) VALUES ($1,$2) ON CONFLICT DO NOTHING",
            &[&tenant, &empty],
        )
        .await
        .map_err(error)?;
        let row = tx
            .query_one(
                "SELECT state FROM dogrs_queue_state_v1 WHERE tenant=$1 FOR UPDATE",
                &[&tenant],
            )
            .await
            .map_err(error)?;
        let mut state: TenantState = serde_json::from_value(row.get(0)).map_err(error)?;
        let result = state.apply(tenant, op)?;
        let value = serde_json::to_value(state).map_err(error)?;
        tx.execute(
            "UPDATE dogrs_queue_state_v1 SET state=$2 WHERE tenant=$1",
            &[&tenant, &value],
        )
        .await
        .map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok(result)
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        Ok(self
            .client()
            .await?
            .query("SELECT tenant FROM dogrs_queue_state_v1", &[])
            .await
            .map_err(error)?
            .into_iter()
            .map(|r| r.get(0))
            .collect())
    }
}
