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
}
