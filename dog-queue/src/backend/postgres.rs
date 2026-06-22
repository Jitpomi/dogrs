//! PostgreSQL backend implementation for Dog Queue
//! This implementation satisfies the `QueueBackend` trait when the `postgres` feature is enabled.

#[cfg(feature = "postgres")]
mod postgres_impl {
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use futures::stream;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_postgres::{Client, NoTls};
    use uuid::Uuid;

    use crate::{
        backend::{BoxStream, QueueBackend, ReapOutcome},
        types::LeaseToken,
        JobEvent, JobId, JobMessage, JobRecord, JobStatus, LeasedJob, QueueCapabilities,
        QueueCtx, QueueError, QueueResult,
    };

    #[cfg(feature = "postgres")]
    #[derive(Clone)]
    pub struct PostgresConfig {
        pub connection_string: String,
    }

    #[cfg(feature = "postgres")]
    pub struct PostgresBackend {
        client: Arc<Mutex<Client>>, // Wrap in Arc<Mutex> for shared mutable access across tasks
    }

    #[cfg(feature = "postgres")]
    impl PostgresBackend {
        pub async fn new(cfg: PostgresConfig) -> QueueResult<Self> {
            let (client, connection) = tokio_postgres::connect(&cfg.connection_string, NoTls)
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            // Spawn the connection object to drive the I/O.
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("Postgres connection error: {}", e);
                }
            });
            // Ensure table exists and has all fields (simple migration)
            client.batch_execute("
                CREATE TABLE IF NOT EXISTS jobs (
                    id UUID PRIMARY KEY,
                    payload JSONB NOT NULL,
                    status TEXT NOT NULL,
                    lease_token TEXT,
                    run_at TIMESTAMPTZ,
                    error TEXT
                );
                ALTER TABLE jobs ADD COLUMN IF NOT EXISTS lease_until TIMESTAMPTZ;
                ALTER TABLE jobs ADD COLUMN IF NOT EXISTS attempt INT DEFAULT 0;
                ALTER TABLE jobs ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ DEFAULT now();
                ALTER TABLE jobs ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ DEFAULT now();
                ALTER TABLE jobs ADD COLUMN IF NOT EXISTS result TEXT;
            ").await.map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            Ok(Self { client: Arc::new(Mutex::new(client)) })
        }
    }

    #[cfg(feature = "postgres")]
    #[async_trait]
    impl QueueBackend for PostgresBackend {
        async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
            let payload = serde_json::to_value(&message)
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            let job_id = Uuid::new_v4();
            let client = self.client.clone();
            let lock = client.lock().await;
            let stmt = "INSERT INTO jobs (id, payload, status, attempt, created_at, updated_at, run_at) VALUES ($1, $2, $3, $4, $5, $6, $7)";
            let now = Utc::now();
            let run_at = message.run_at;
            lock.execute(stmt, &[&job_id, &payload, &"enqueued".to_string(), &0_i32, &now, &now, &run_at])
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            Ok(job_id.to_string().into())
        }

        async fn dequeue(
            &self,
            ctx: QueueCtx,
            queues: &[&str],
        ) -> QueueResult<Option<LeasedJob>> {
            let client = self.client.clone();
            let lock = client.lock().await;
            
            let queues_vec: Vec<String> = queues.iter().map(|s| s.to_string()).collect();

            // Find next ready job (run_at <= now or null) and lock the row
            let stmt = "
                SELECT id, payload, attempt, created_at FROM jobs
                WHERE (status = 'enqueued' OR status = 'retrying')
                    AND (run_at IS NULL OR run_at <= now())
                    AND (payload->>'queue' = ANY($1))
                ORDER BY id
                FOR UPDATE SKIP LOCKED
                LIMIT 1;
            ";
            if let Some(row) = lock.query_opt(stmt, &[&queues_vec]).await.map_err(|e| QueueError::BackendUnsupported(e.to_string()))? {
                let job_id: Uuid = row.get(0);
                let payload: serde_json::Value = row.get(1);
                let attempt: i32 = row.get(2);
                let created_at: DateTime<Utc> = row.get(3);

                let message: JobMessage = serde_json::from_value(payload)
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
                
                let lease_token = LeaseToken::new();
                let lease_until = Utc::now() + chrono::Duration::seconds(300);
                let new_attempt = attempt + 1;
                let now = Utc::now();

                // Mark as leased
                let update = "UPDATE jobs SET status = $1, lease_token = $2, lease_until = $3, attempt = $4, updated_at = $5 WHERE id = $6";
                lock.execute(update, &[&"processing".to_string(), &lease_token.as_str().to_string(), &lease_until, &new_attempt, &now, &job_id])
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

                let record = JobRecord {
                    job_id: JobId::from(job_id.to_string()),
                    tenant_id: ctx.tenant_id.clone(),
                    message,
                    status: JobStatus::Processing { lease_until },
                    attempt: new_attempt as u32,
                    created_at,
                    updated_at: now,
                    last_error: None,
                    result: None,
                    lease_token: Some(lease_token.clone()),
                };

                let leased = LeasedJob {
                    record,
                    lease_token,
                    lease_until,
                };
                Ok(Some(leased))
            } else {
                Ok(None)
            }
        }

        async fn ack_complete(
            &self,
            _ctx: QueueCtx,
            job_id: JobId,
            lease_token: LeaseToken,
            _result_ref: Option<String>,
        ) -> QueueResult<()> {
            let job_uuid = Uuid::parse_str(job_id.as_str()).map_err(|e| QueueError::InvalidConfig(e.to_string()))?;
            let client = self.client.clone();
            let lock = client.lock().await;
            // Verify lease token
            let verify = "SELECT lease_token FROM jobs WHERE id = $1";
            let stored: Option<String> = lock.query_opt(verify, &[&job_uuid])
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?
                .and_then(|r| r.get(0));
            if stored.as_deref() != Some(lease_token.as_str()) {
                return Err(QueueError::InvalidLeaseToken { job_id });
            }
            // Delete the job
            let del = "DELETE FROM jobs WHERE id = $1";
            lock.execute(del, &[&job_uuid])
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            Ok(())
        }

        async fn ack_fail(
            &self,
            _ctx: QueueCtx,
            job_id: JobId,
            lease_token: LeaseToken,
            error: String,
            retry_at: Option<DateTime<Utc>>,
        ) -> QueueResult<()> {
            let job_uuid = Uuid::parse_str(job_id.as_str()).map_err(|e| QueueError::InvalidConfig(e.to_string()))?;
            let client = self.client.clone();
            let lock = client.lock().await;
            // Verify lease token
            let verify = "SELECT lease_token FROM jobs WHERE id = $1";
            let stored: Option<String> = lock.query_opt(verify, &[&job_uuid])
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?
                .and_then(|r| r.get(0));
            if stored.as_deref() != Some(lease_token.as_str()) {
                return Err(QueueError::InvalidLeaseToken { job_id });
            }
            // Update status to failed and store error
            let now = Utc::now();
            if let Some(rt) = retry_at {
                let stmt = "UPDATE jobs SET status = $1, error = $2, run_at = $3, lease_token = NULL, lease_until = NULL, updated_at = $4 WHERE id = $5";
                lock.execute(stmt, &[&"retrying".to_string(), &error, &rt, &now, &job_uuid])
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            } else {
                let stmt = "UPDATE jobs SET status = $1, error = $2, lease_token = NULL, lease_until = NULL, updated_at = $3 WHERE id = $4";
                lock.execute(stmt, &[&"failed".to_string(), &error, &now, &job_uuid])
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            }
            Ok(())
        }

        async fn heartbeat_extend(
            &self,
            _ctx: QueueCtx,
            _job_id: JobId,
            _lease_token: LeaseToken,
            _extra_time: std::time::Duration,
        ) -> QueueResult<()> {
            Err(QueueError::BackendUnsupported("heartbeat_extend not supported".into()))
        }

        async fn cancel(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
            let job_uuid = Uuid::parse_str(job_id.as_str()).map_err(|e| QueueError::InvalidConfig(e.to_string()))?;
            let client = self.client.clone();
            let lock = client.lock().await;
            let del = "DELETE FROM jobs WHERE id = $1";
            let affected = lock.execute(del, &[&job_uuid])
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            Ok(affected > 0)
        }

        async fn get_status(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus> {
            let job_uuid = Uuid::parse_str(job_id.as_str()).map_err(|e| QueueError::InvalidConfig(e.to_string()))?;
            let client = self.client.clone();
            let lock = client.lock().await;
            let stmt = "SELECT status, lease_until, run_at, error FROM jobs WHERE id = $1";
            let row = lock.query_opt(stmt, &[&job_uuid])
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            if let Some(r) = row {
                let status_str: String = r.get(0);
                let lease_until: Option<DateTime<Utc>> = r.get(1);
                let run_at: Option<DateTime<Utc>> = r.get(2);
                let error: Option<String> = r.get(3);
                
                let status = match status_str.as_str() {
                    "enqueued" => JobStatus::Enqueued,
                    "processing" => JobStatus::Processing {
                        lease_until: lease_until.unwrap_or_else(Utc::now),
                    },
                    "retrying" => JobStatus::Retrying {
                        retry_at: run_at.unwrap_or_else(Utc::now),
                    },
                    "completed" => JobStatus::Completed {
                        completed_at: Utc::now(),
                    },
                    "failed" => JobStatus::Failed {
                        failed_at: Utc::now(),
                        error: error.unwrap_or_default(),
                    },
                    "canceled" => JobStatus::Canceled {
                        canceled_at: Utc::now(),
                    },
                    _ => JobStatus::Enqueued,
                };
                Ok(status)
            } else {
                Err(QueueError::JobNotFound(job_id))
            }
        }

        async fn get_record(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
            let job_uuid = Uuid::parse_str(job_id.as_str()).map_err(|e| QueueError::InvalidConfig(e.to_string()))?;
            let client = self.client.clone();
            let lock = client.lock().await;
            let stmt = "SELECT payload, status, lease_token, lease_until, attempt, created_at, updated_at, error, result, run_at FROM jobs WHERE id = $1";
            let row = lock.query_opt(stmt, &[&job_uuid])
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            if let Some(r) = row {
                let payload: serde_json::Value = r.get(0);
                let status_str: String = r.get(1);
                let lease_token_str: Option<String> = r.get(2);
                let lease_until: Option<DateTime<Utc>> = r.get(3);
                let attempt: i32 = r.get(4);
                let created_at: DateTime<Utc> = r.get(5);
                let updated_at: DateTime<Utc> = r.get(6);
                let error: Option<String> = r.get(7);
                let result: Option<String> = r.get(8);
                let run_at: Option<DateTime<Utc>> = r.get(9);

                let message: JobMessage = serde_json::from_value(payload)
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

                let lease_token = lease_token_str.map(LeaseToken::from);

                let status = match status_str.as_str() {
                    "enqueued" => JobStatus::Enqueued,
                    "processing" => JobStatus::Processing {
                        lease_until: lease_until.unwrap_or_else(Utc::now),
                    },
                    "retrying" => JobStatus::Retrying {
                        retry_at: run_at.unwrap_or_else(Utc::now),
                    },
                    "completed" => JobStatus::Completed {
                        completed_at: updated_at,
                    },
                    "failed" => JobStatus::Failed {
                        failed_at: updated_at,
                        error: error.clone().unwrap_or_default(),
                    },
                    "canceled" => JobStatus::Canceled {
                        canceled_at: updated_at,
                    },
                    _ => JobStatus::Enqueued,
                };

                Ok(JobRecord {
                    job_id,
                    tenant_id: ctx.tenant_id.clone(),
                    message,
                    status,
                    attempt: attempt as u32,
                    created_at,
                    updated_at,
                    last_error: error,
                    result,
                    lease_token,
                })
            } else {
                Err(QueueError::JobNotFound(job_id))
            }
        }

        fn event_stream(&self, _ctx: QueueCtx) -> BoxStream<JobEvent> {
            let s = stream::empty();
            Box::pin(s) as BoxStream<JobEvent>
        }

        async fn reclaim_expired_leases(&self) -> QueueResult<Vec<ReapOutcome>> {
            Ok(vec![])
        }

        fn capabilities(&self) -> QueueCapabilities {
            QueueCapabilities {
                lease_extend: false,
                ..Default::default()
            }
        }
    }
}

#[cfg(feature = "postgres")]
pub use postgres_impl::{PostgresBackend, PostgresConfig};

