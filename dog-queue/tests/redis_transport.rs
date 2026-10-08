#![cfg(feature = "redis")]

use redis::{aio::MultiplexedConnection, IntoConnectionInfo};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

// Regress redis-rs #1955 through its public API. Both directions exceed the
// socket capacity; a writer-only driver cannot progress until it reads replies.
// No real server, timing-dependent disk stall, or provider credential is needed.
#[tokio::test]
async fn large_concurrent_requests_keep_reading_while_writes_are_blocked() {
    let (client_io, server_io) = tokio::io::duplex(256);
    let server = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(server_io);
        let mut reader = BufReader::new(reader);
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).await.unwrap() == 0 {
                break;
            }
            let count: usize = header.trim().strip_prefix('*').unwrap().parse().unwrap();
            let mut args = Vec::new();
            for _ in 0..count {
                header.clear();
                reader.read_line(&mut header).await.unwrap();
                let len: usize = header.trim().strip_prefix('$').unwrap().parse().unwrap();
                let mut value = vec![0; len + 2];
                reader.read_exact(&mut value).await.unwrap();
                value.truncate(len);
                args.push(value);
            }
            let response = if args[0].eq_ignore_ascii_case(b"ECHO") {
                let mut response = format!("${}\r\n", args[1].len()).into_bytes();
                response.extend_from_slice(&args[1]);
                response.extend_from_slice(b"\r\n");
                response
            } else {
                // Connection setup: CLIENT SETINFO/SETNAME and SELECT.
                b"+OK\r\n".to_vec()
            };
            if writer.write_all(&response).await.is_err() {
                break;
            }
        }
    });
    let (ready, connection) = tokio::sync::oneshot::channel();
    let driver = tokio::spawn(async move {
        let info = "redis://127.0.0.1/".into_connection_info().unwrap();
        let (connection, driver) = MultiplexedConnection::new(info.redis_settings(), client_io)
            .await
            .unwrap();
        ready.send(connection).unwrap();
        driver.await;
    });
    let connection = connection.await.unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for value in 0..16u8 {
        let mut connection = connection.clone();
        tasks.spawn(async move {
            let payload = vec![value; 65536];
            let reply: Vec<u8> = redis::cmd("ECHO")
                .arg(&payload)
                .query_async(&mut connection)
                .await
                .unwrap();
            assert_eq!(reply, payload);
        });
    }
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    })
    .await;
    tasks.abort_all();
    driver.abort();
    server.abort();
    assert!(result.is_ok(), "duplex request/reply transport deadlocked");
}
