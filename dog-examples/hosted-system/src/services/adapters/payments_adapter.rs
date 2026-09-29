//! PostgreSQL adapter for synthetic payment validation records.

use tokio_postgres::Client;

pub struct PaymentsAdapter;

impl PaymentsAdapter {
    /// Records a payment execution attempt and returns the updated attempt count.
    pub async fn record_attempt(
        client: &Client,
        tenant: &str,
        invoice: &str,
    ) -> Result<i32, tokio_postgres::Error> {
        let row = client
            .query_one(
                "INSERT INTO dogrs_validation_attempts (tenant,invoice,attempts) VALUES ($1,$2,1) ON CONFLICT (tenant,invoice) DO UPDATE SET attempts=dogrs_validation_attempts.attempts+1 RETURNING attempts",
                &[&tenant, &invoice],
            )
            .await?;
        Ok(row.get(0))
    }

    /// Records a committed payment effect idempotently in the database.
    pub async fn record_effect(
        client: &Client,
        tenant: &str,
        invoice: &str,
        worker_id: &str,
    ) -> Result<u64, tokio_postgres::Error> {
        client
            .execute(
                "INSERT INTO dogrs_validation_effects (tenant,invoice,worker) VALUES ($1,$2,$3) ON CONFLICT (tenant,invoice) DO NOTHING",
                &[&tenant, &invoice, &worker_id],
            )
            .await
    }

    /// Initializes the database schema for the synthetic payment validation records.
    pub async fn init_schema(client: &Client) -> anyhow::Result<()> {
        client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS dogrs_validation_attempts (\
                    tenant TEXT NOT NULL, \
                    invoice TEXT NOT NULL, \
                    attempts INTEGER NOT NULL, \
                    PRIMARY KEY (tenant, invoice)\
                ); \
                CREATE TABLE IF NOT EXISTS dogrs_validation_effects (\
                    tenant TEXT NOT NULL, \
                    invoice TEXT NOT NULL, \
                    worker TEXT NOT NULL, \
                    created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
                    PRIMARY KEY (tenant, invoice)\
                );",
            )
            .await?;
        Ok(())
    }

    /// Inspects payment validation attempts and effects for a tenant.
    pub async fn inspect_schema(
        client: &Client,
        tenant: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let rows = client
            .query(
                "SELECT a.invoice, a.attempts, e.worker \
                FROM dogrs_validation_attempts a \
                LEFT JOIN dogrs_validation_effects e USING (tenant, invoice) \
                WHERE a.tenant = $1 \
                ORDER BY a.invoice",
                &[&tenant],
            )
            .await?;
        let result = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "invoice": r.get::<_, String>(0),
                    "attempts": r.get::<_, i32>(1),
                    "worker": r.get::<_, Option<String>>(2),
                })
            })
            .collect();
        Ok(result)
    }
}
