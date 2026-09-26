//! Redis backend implementation for Dog Queue
//! This implementation satisfies the `QueueBackend` trait when the `redis` feature is enabled.

#[cfg(feature = "redis")]
mod redis_impl {
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use futures::stream;
    use redis::AsyncCommands;
    use redis::aio::ConnectionManager;
    use std::time::Duration;

    use crate::{
        backend::{BoxStream, QueueBackend, ReapOutcome},
        types::LeaseToken,
        JobEvent, JobId, JobMessage, JobRecord, JobStatus, LeasedJob, QueueCapabilities,
        QueueCtx, QueueError, QueueResult,
    };

    #[cfg(feature = "redis")]
    #[derive(Clone)]
    pub struct RedisConfig {
        pub connection_string: String,
    }

    #[cfg(feature = "redis")]
    pub struct RedisBackend {
        manager: ConnectionManager,
    }

    #[cfg(feature = "redis")]
    impl RedisBackend {
        pub async fn new(cfg: RedisConfig) -> QueueResult<Self> {
            let client = redis::Client::open(cfg.connection_string.as_str())
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            let manager = client
                .get_connection_manager()
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            Ok(Self { manager })
        }

        fn job_key(queue: &str) -> String {
            format!("queue:{}", queue)
        }
    }

    #[cfg(feature = "redis")]
    #[async_trait]
    impl QueueBackend for RedisBackend {
        async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
            let mut conn = self.manager.clone();
            let payload = serde_json::to_string(&message)
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            let key = Self::job_key(&message.queue);
            let job_id = uuid::Uuid::new_v4().to_string();
            let now_str = Utc::now().to_rfc3339();

            // Store payload as a hash: id -> payload, status, attempt, created_at, updated_at
            let _: () = conn
                .hset_multiple(
                    format!("job:{}", job_id),
                    &[
                        ("payload", payload),
                        ("status", "enqueued".to_string()),
                        ("attempt", "0".to_string()),
                        ("created_at", now_str.clone()),
                        ("updated_at", now_str),
                    ],
                )
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            // Push the id onto the queue list
            let _: () = conn
                .rpush(key, job_id.clone())
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            Ok(job_id.into())
        }

        async fn dequeue(
            &self,
            ctx: QueueCtx,
            queues: &[&str],
        ) -> QueueResult<Option<LeasedJob>> {
            let mut conn = self.manager.clone();
            for q in queues {
                let key = Self::job_key(q);
                let job_id_opt: Option<String> = conn
                    .lpop(key, None)
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
                if let Some(job_id) = job_id_opt {
                    let job_key = format!("job:{}", job_id);
                    
                    let fields: std::collections::HashMap<String, String> = conn
                        .hgetall(&job_key)
                        .await
                        .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

                    if fields.is_empty() {
                        continue;
                    }

                    let payload_str = fields.get("payload")
                        .ok_or_else(|| QueueError::BackendUnsupported("Missing payload field".into()))?;
                    let message: JobMessage = serde_json::from_str(payload_str)
                        .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

                    let lease_token = LeaseToken::new();
                    let lease_until = Utc::now() + chrono::Duration::seconds(300);
                    let now_str = Utc::now().to_rfc3339();
                    let lease_until_str = lease_until.to_rfc3339();

                    // Get current attempt count and increment it
                    let attempt = fields.get("attempt")
                        .and_then(|a| a.parse::<u32>().ok())
                        .unwrap_or(0) + 1;

                    // Mark as leased (processing)
                    let _: () = conn
                        .hset_multiple(
                            &job_key,
                            &[
                                ("status", "processing".to_string()),
                                ("lease_token", lease_token.as_str().to_string()),
                                ("lease_until", lease_until_str),
                                ("attempt", attempt.to_string()),
                                ("updated_at", now_str),
                            ],
                        )
                        .await
                        .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

                    let created_at = fields.get("created_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);

                    let record = JobRecord {
                        job_id: JobId::from(job_id),
                        tenant_id: ctx.tenant_id.clone(),
                        message,
                        status: JobStatus::Processing { lease_until },
                        attempt,
                        created_at,
                        updated_at: Utc::now(),
                        last_error: fields.get("error").cloned(),
                        result: fields.get("result").cloned(),
                        lease_token: Some(lease_token.clone()),
                    };

                    let leased = LeasedJob {
                        record,
                        lease_token,
                        lease_until,
                    };
                    return Ok(Some(leased));
                }
            }
            Ok(None)
        }

        async fn ack_complete(
            &self,
            _ctx: QueueCtx,
            job_id: JobId,
            lease_token: LeaseToken,
            _result_ref: Option<String>,
        ) -> QueueResult<()> {
            let mut conn = self.manager.clone();
            let job_key = format!("job:{}", job_id.as_str());
            let stored_token: Option<String> = conn
                .hget(&job_key, "lease_token")
                .await
                .ok();
            if stored_token.as_deref() != Some(lease_token.as_str()) {
                return Err(QueueError::InvalidLeaseToken { job_id });
            }
            let _: () = conn
                .del(job_key)
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
            let mut conn = self.manager.clone();
            let job_key = format!("job:{}", job_id.as_str());
            let stored_token: Option<String> = conn
                .hget(&job_key, "lease_token")
                .await
                .ok();
            if stored_token.as_deref() != Some(lease_token.as_str()) {
                return Err(QueueError::InvalidLeaseToken { job_id });
            }
            let now_str = Utc::now().to_rfc3339();
            if let Some(rt) = retry_at {
                let rt_str = rt.to_rfc3339();
                let _: () = conn
                    .hset_multiple(
                        &job_key,
                        &[
                            ("status", "retrying".to_string()),
                            ("error", error),
                            ("retry_at", rt_str),
                            ("updated_at", now_str),
                        ],
                    )
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
                // Clear lease_token and lease_until
                let _: () = conn.hdel(&job_key, "lease_token").await.map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
                let _: () = conn.hdel(&job_key, "lease_until").await.map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

                // Re-push onto queue for retry
                let payload_json: Option<String> = conn.hget(&job_key, "payload").await.ok();
                let queue_name = if let Some(ref p) = payload_json {
                    serde_json::from_str::<JobMessage>(p)
                        .map(|m| m.queue)
                        .unwrap_or_else(|_| "default".to_string())
                } else {
                    "default".to_string()
                };
                let key = Self::job_key(&queue_name);
                let _: () = conn
                    .rpush(key, job_id.as_str())
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            } else {
                let _: () = conn
                    .hset_multiple(
                        &job_key,
                        &[
                            ("status", "failed".to_string()),
                            ("error", error),
                            ("failed_at", now_str.clone()),
                            ("updated_at", now_str),
                        ],
                    )
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
                // Clear lease_token and lease_until
                let _: () = conn.hdel(&job_key, "lease_token").await.map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
                let _: () = conn.hdel(&job_key, "lease_until").await.map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            }
            Ok(())
        }

        async fn heartbeat_extend(
            &self,
            _ctx: QueueCtx,
            _job_id: JobId,
            _lease_token: LeaseToken,
            _extra_time: Duration,
        ) -> QueueResult<()> {
            Err(QueueError::BackendUnsupported("heartbeat_extend not supported".into()))
        }

        async fn cancel(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
            let mut conn = self.manager.clone();
            let deleted: u64 = conn
                .del(format!("job:{}", job_id.as_str()))
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;
            Ok(deleted > 0)
        }

        async fn get_status(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus> {
            let mut conn = self.manager.clone();
            let fields: std::collections::HashMap<String, String> = conn
                .hgetall(format!("job:{}", job_id.as_str()))
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

            if fields.is_empty() {
                return Err(QueueError::JobNotFound(job_id));
            }

            let status_str = fields.get("status")
                .map(|s| s.as_str())
                .unwrap_or("enqueued");

            let lease_until = fields.get("lease_until")
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc));

            let status = match status_str {
                "enqueued" => JobStatus::Enqueued,
                "processing" => {
                    let until = lease_until.unwrap_or_else(Utc::now);
                    JobStatus::Processing { lease_until: until }
                }
                "retrying" => {
                    let retry_at = fields.get("retry_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    JobStatus::Retrying { retry_at }
                }
                "completed" => {
                    let completed_at = fields.get("completed_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    JobStatus::Completed { completed_at }
                }
                "failed" => {
                    let failed_at = fields.get("failed_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    let error = fields.get("error").cloned().unwrap_or_default();
                    JobStatus::Failed { failed_at, error }
                }
                "canceled" => {
                    let canceled_at = fields.get("canceled_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    JobStatus::Canceled { canceled_at }
                }
                _ => JobStatus::Enqueued,
            };

            Ok(status)
        }

        async fn get_record(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
            let mut conn = self.manager.clone();
            let fields: std::collections::HashMap<String, String> = conn
                .hgetall(format!("job:{}", job_id.as_str()))
                .await
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

            if fields.is_empty() {
                return Err(QueueError::JobNotFound(job_id));
            }

            let payload_str = fields.get("payload")
                .ok_or_else(|| QueueError::BackendUnsupported("Missing payload field".into()))?;
            let message: JobMessage = serde_json::from_str(payload_str)
                .map_err(|e| QueueError::BackendUnsupported(e.to_string()))?;

            let status_str = fields.get("status")
                .map(|s| s.as_str())
                .unwrap_or("enqueued");

            let lease_token = fields.get("lease_token")
                .map(|t| LeaseToken::from(t.clone()));

            let lease_until = fields.get("lease_until")
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc));

            let status = match status_str {
                "enqueued" => JobStatus::Enqueued,
                "processing" => {
                    let until = lease_until.unwrap_or_else(Utc::now);
                    JobStatus::Processing { lease_until: until }
                }
                "retrying" => {
                    let retry_at = fields.get("retry_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    JobStatus::Retrying { retry_at }
                }
                "completed" => {
                    let completed_at = fields.get("completed_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    JobStatus::Completed { completed_at }
                }
                "failed" => {
                    let failed_at = fields.get("failed_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    let error = fields.get("error").cloned().unwrap_or_default();
                    JobStatus::Failed { failed_at, error }
                }
                "canceled" => {
                    let canceled_at = fields.get("canceled_at")
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    JobStatus::Canceled { canceled_at }
                }
                _ => JobStatus::Enqueued,
            };

            let attempt = fields.get("attempt")
                .and_then(|a| a.parse::<u32>().ok())
                .unwrap_or(0);

            let created_at = fields.get("created_at")
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);

            let updated_at = fields.get("updated_at")
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);

            let last_error = fields.get("error").cloned();
            let result = fields.get("result").cloned();

            Ok(JobRecord {
                job_id,
                tenant_id: ctx.tenant_id.clone(),
                message,
                status,
                attempt,
                created_at,
                updated_at,
                last_error,
                result,
                lease_token,
            })
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

#[cfg(feature = "redis")]
pub use redis_impl::{RedisBackend, RedisConfig};

