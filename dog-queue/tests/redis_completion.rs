#![cfg(feature = "redis")]
use dog_queue::{
    backend::redis::{RedisBackend, RedisConfig},
    JobMessage, JobStatus, LeaseToken, QueueBackend, QueueCtx, QueueError,
};

#[tokio::test]
#[ignore = "requires disposable Redis"]
async fn atomic_completion_preserves_metadata_and_fences_competing_writers() {
    let config = RedisConfig {
        connection_string: std::env::var("DOGRS_REDIS_URL").unwrap(),
    };
    let mut clock = redis::Client::open(config.connection_string.clone())
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let a = RedisBackend::new(config.clone()).await.unwrap();
    let b = RedisBackend::new(config).await.unwrap();
    let ctx = QueueCtx::new(format!("completion-{}", uuid::Uuid::new_v4()));
    // Exercise cjson/serde escaping, empty payload arrays, optional results, and
    // reuse of exactly the same dedupe key after terminal completion.
    let unusual = "quotes\" slash/ backslash\\ \0\u{1}\u{8}\u{c}\n\r\t café 🐕";
    for bytes in [vec![], vec![255; 65536]] {
        let message =
            JobMessage::new(unusual, bytes.clone(), unusual, unusual).with_idempotency_key(unusual);
        let id = a.enqueue(ctx.clone(), message.clone()).await.unwrap();
        let job = b.dequeue(ctx.clone(), &[unusual]).await.unwrap().unwrap();
        let before = b.get_record(ctx.clone(), id.clone()).await.unwrap();
        assert!(matches!(
            a.ack_complete(ctx.clone(), id.clone(), LeaseToken::new(), None)
                .await,
            Err(QueueError::InvalidLeaseToken { .. })
        ));
        assert!(matches!(
            a.ack_complete(
                ctx.clone(),
                id.clone(),
                job.lease_token.clone(),
                Some("x".repeat(4096))
            )
            .await,
            Err(QueueError::InvalidConfig(_))
        ));
        assert!(matches!(
            a.get_status(ctx.clone(), id.clone()).await.unwrap(),
            JobStatus::Processing { .. }
        ));
        let (secs, micros): (i64, i64) = redis::cmd("TIME").query_async(&mut clock).await.unwrap();
        let start = chrono::DateTime::from_timestamp_millis(secs * 1000 + micros / 1000).unwrap();
        let (left, right) = tokio::join!(
            a.ack_complete(
                ctx.clone(),
                id.clone(),
                job.lease_token.clone(),
                Some(unusual.into())
            ),
            b.ack_complete(
                ctx.clone(),
                id.clone(),
                job.lease_token,
                Some(unusual.into())
            )
        );
        assert_ne!(left.is_ok(), right.is_ok());
        assert!(matches!(
            left.err().or(right.err()),
            Some(QueueError::JobAlreadyTerminal)
        ));
        let after = a.get_record(ctx.clone(), id.clone()).await.unwrap();
        assert_eq!(after.message, before.message);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(after.attempt, before.attempt);
        assert_eq!(after.result.as_deref(), Some(unusual));
        let (secs, micros): (i64, i64) = redis::cmd("TIME").query_async(&mut clock).await.unwrap();
        let end = chrono::DateTime::from_timestamp_millis(secs * 1000 + micros / 1000).unwrap();
        assert!(
            after.updated_at >= start,
            "server timestamp before completion"
        );
        assert!(after.updated_at <= end, "server timestamp after completion");
        assert!(
            matches!(after.status, JobStatus::Completed { completed_at } if completed_at == after.updated_at)
        );
        let next = a.enqueue(ctx.clone(), message).await.unwrap();
        assert_ne!(
            next, id,
            "completion must release the exact escaped dedupe key"
        );
        let job = a.dequeue(ctx.clone(), &[unusual]).await.unwrap().unwrap();
        b.ack_complete(ctx.clone(), next.clone(), job.lease_token, None)
            .await
            .unwrap();
        assert_eq!(a.get_record(ctx.clone(), next).await.unwrap().result, None);
    }
}

#[tokio::test]
#[ignore = "requires disposable Redis"]
async fn completion_clock_conversion_handles_calendar_boundaries() {
    let url = std::env::var("DOGRS_REDIS_URL").unwrap();
    let backend = RedisBackend::new(RedisConfig {
        connection_string: url.clone(),
    })
    .await
    .unwrap();
    let tenant = format!("calendar-{}", uuid::Uuid::new_v4());
    let ctx = QueueCtx::new(&tenant);
    let id = backend
        .enqueue(ctx.clone(), JobMessage::new("clock", vec![], "bytes", "q"))
        .await
        .unwrap();
    let lease = backend.dequeue(ctx, &["q"]).await.unwrap().unwrap();
    let prefix = format!(
        "{{dogrs-queue-v2:{}}}",
        tenant
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let mut connection = redis::Client::open(url)
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let raw: String = redis::cmd("HGET")
        .arg(format!("{prefix}:meta"))
        .arg(id.as_str())
        .query_async(&mut connection)
        .await
        .unwrap();
    // Only this test replaces TIME, avoiding dependence on the host calendar.
    let script = format!("local actual=redis; local redis={{call=function(cmd,...) if cmd=='TIME' then return {{ARGV[4],ARGV[5]}} else return actual.call(cmd,...) end end}};\n{}", include_str!("../src/backend/redis_complete.lua"));
    for date in [
        "1970-01-01T00:00:00.000Z",
        "2000-02-29T23:59:59.999Z",
        "2026-12-31T23:59:59.001Z",
        "2100-03-01T00:00:00.123Z",
    ] {
        let time = chrono::DateTime::parse_from_rfc3339(date).unwrap();
        let _: () = redis::pipe()
            .cmd("HSET")
            .arg(format!("{prefix}:meta"))
            .arg(id.as_str())
            .arg(&raw)
            .ignore()
            .cmd("ZADD")
            .arg(format!("{prefix}:leases"))
            .arg(time.timestamp_millis() + 10000)
            .arg(id.as_str())
            .ignore()
            .query_async(&mut connection)
            .await
            .unwrap();
        let status: i32 = redis::Script::new(&script)
            .key(format!("{prefix}:meta"))
            .key(format!("{prefix}:leases"))
            .key(format!("{prefix}:terminal"))
            .key(format!("{prefix}:dedupe"))
            .arg(id.as_str())
            .arg(lease.lease_token.as_str())
            .arg("null")
            .arg(time.timestamp())
            .arg(time.timestamp_subsec_micros())
            .invoke_async(&mut connection)
            .await
            .unwrap();
        assert_eq!(status, 0);
        let raw: String = redis::cmd("HGET")
            .arg(format!("{prefix}:meta"))
            .arg(id.as_str())
            .query_async(&mut connection)
            .await
            .unwrap();
        let row: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let actual =
            chrono::DateTime::parse_from_rfc3339(row["record"]["updated_at"].as_str().unwrap())
                .unwrap();
        assert_eq!(actual, time);
    }
}
