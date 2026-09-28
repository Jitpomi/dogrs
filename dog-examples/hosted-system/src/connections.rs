use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
#[cfg(any(feature = "sqs", feature = "kafka-rust"))]
use anyhow::Context;
use dog_queue::{QueueBackend, QueueCtx};
use serde_json::json;
use tokio_postgres::Client;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::runner::{env, run, tenant, LEASE};

#[cfg(any(
    feature = "rabbitmq",
    feature = "kafka",
    feature = "kafka-rust",
    feature = "sqs",
    feature = "pubsub"
))]
use dog_queue::backend::broker::Notifications;
#[cfg(feature = "kafka")]
use dog_queue::backend::kafka::RdKafkaBackend;
#[cfg(feature = "nats")]
use dog_queue::backend::nats::NatsBackend;
use dog_queue::backend::postgres::{PostgresBackend, PostgresConfig};
#[cfg(feature = "redis")]
use dog_queue::backend::redis::{RedisBackend, RedisConfig};
#[cfg(feature = "rabbitmq")]
use dog_queue::backend::{broker::JobLedger, rabbitmq::RabbitMqBackend};

fn secret(name: &str) -> Result<String> {
    let dir = PathBuf::from(env("DOGRS_SECRETS_DIR")?);
    Ok(std::fs::read_to_string(dir.join(name))?.trim().to_owned())
}
fn path(name: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(env("DOGRS_SECRETS_DIR")?).join(name))
}
fn pg_tls() -> Result<MakeRustlsConnect> {
    let mut roots = rustls::RootCertStore::empty();
    let bytes = std::fs::read(path("aiven-ca.pem")?)?;
    for cert in rustls_pemfile::certs(&mut bytes.as_slice()) {
        roots.add(cert?)?;
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}
pub async fn postgres_client() -> Result<Client> {
    let uri = secret("postgres.uri")?;
    anyhow::ensure!(
        uri.contains("sslmode=require"),
        "hosted PostgreSQL must require TLS"
    );
    let (client, connection) = tokio_postgres::connect(&uri, pg_tls()?).await?;
    tokio::spawn(async move {
        if connection.await.is_err() {
            eprintln!("POSTGRES_CONNECTION_CLOSED");
        }
    });
    Ok(client)
}
async fn postgres() -> Result<PostgresBackend> {
    Ok(PostgresBackend::new_with_tls(
        PostgresConfig {
            connection_string: secret("postgres.uri")?,
        },
        pg_tls()?,
    )
    .await?
    .with_lease_duration(LEASE))
}
#[cfg(any(
    feature = "rabbitmq",
    feature = "kafka",
    feature = "kafka-rust",
    feature = "sqs",
    feature = "pubsub"
))]
async fn notification_probe(n: &impl Notifications) -> Result<()> {
    // A broker outage intentionally does not fail enqueue; prove the broker really works
    // separately so ledger polling cannot mask broken TLS, auth, permissions or routing.
    n.publish().await?;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if n.receive().await? {
                return Ok::<_, dog_queue::QueueError>(());
            }
        }
    })
    .await??;
    println!("BROKER_NOTIFICATION_VERIFIED");
    Ok(())
}
pub async fn dispatch(role: &str) -> Result<()> {
    let kind = env("DOGRS_BACKEND")?;
    if role == "migrate" {
        anyhow::ensure!(kind == "postgres", "migration role is PostgreSQL-specific");
        PostgresBackend::new_with_tls_options(
            PostgresConfig {
                connection_string: secret("postgres.uri")?,
            },
            pg_tls()?,
            dog_queue::backend::postgres::PostgresOptions {
                migrate_legacy: true,
                operation_timeout: Duration::from_secs(300),
                ..Default::default()
            },
        )
        .await?;
        println!("POSTGRES_OFFLINE_MIGRATION_COMPLETE");
        return Ok(());
    }
    match kind.as_str() {
        "postgres" => run(postgres().await?, role).await,
        #[cfg(feature = "pubsub")]
        "pubsub" => {
            use dog_queue::backend::gcp_pubsub::GcpPubSubBackend;
            use google_cloud_pubsub::client::{Publisher, Subscriber};
            let project = env("DOGRS_GCP_PROJECT")?;
            let endpoint = "https://us-west1-pubsub.googleapis.com";
            let publisher =
                Publisher::builder(format!("projects/{project}/topics/dogrs-validation"))
                    .with_endpoint(endpoint)
                    .build()
                    .await?;
            let subscriber = Subscriber::builder()
                .with_endpoint(endpoint)
                .build()
                .await?;
            let backend = GcpPubSubBackend::new(
                publisher,
                subscriber,
                format!("projects/{project}/subscriptions/dogrs-validation"),
                Arc::new(postgres().await?),
            )?;
            if role == "probe" {
                return notification_probe(backend.notifications()).await;
            }
            run(backend, role).await
        }
        #[cfg(feature = "sqs")]
        "sqs" => {
            use aws_sdk_sqs::config::{BehaviorVersion, Credentials, Region};
            use dog_queue::backend::aws_sqs::AwsSqsBackend;
            let credentials: serde_json::Value = serde_json::from_str(&secret("aws.json")?)?;
            let access = credentials["access_key_id"]
                .as_str()
                .context("missing AWS access key")?;
            let private = credentials["secret_access_key"]
                .as_str()
                .context("missing AWS secret key")?;
            let token = credentials["session_token"].as_str().map(str::to_owned);
            let config = aws_sdk_sqs::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new("us-east-2"))
                .credentials_provider(Credentials::new(access, private, token, None, "dogrs-test"))
                .build();
            let backend = AwsSqsBackend::new(
                aws_sdk_sqs::Client::from_conf(config),
                "https://sqs.us-east-2.amazonaws.com/713005938483/dogrs-validation".into(),
                Arc::new(postgres().await?),
            )?;
            if role == "probe" {
                return notification_probe(backend.notifications()).await;
            }
            run(backend, role).await
        }
        #[cfg(feature = "redis")]
        "redis" => {
            let uri = secret("redis.uri")?;
            anyhow::ensure!(uri.starts_with("rediss://"), "hosted Redis requires TLS");
            if role == "probe" {
                let client = redis::Client::open(uri)?;
                let mut connection = client.get_multiplexed_async_connection().await?;
                let info: String = redis::cmd("INFO")
                    .arg("persistence")
                    .query_async(&mut connection)
                    .await?;
                for line in info.lines().filter(|line| {
                    line.starts_with("aof_enabled:") || line.starts_with("rdb_last_bgsave_status:")
                }) {
                    println!("{line}");
                }
                return Ok(());
            }
            run(
                RedisBackend::new(RedisConfig {
                    connection_string: uri,
                })
                .await?
                .with_lease_duration(LEASE),
                role,
            )
            .await
        }
        #[cfg(feature = "nats")]
        "nats" => {
            let client = async_nats::ConnectOptions::new()
                .credentials_file(path("nats.creds")?)
                .await?
                .require_tls(true)
                .connect("tls://connect.ngs.global")
                .await?;
            let max_payload = client.server_info().max_payload;
            let js = async_nats::jetstream::new(client);
            let name = "dogrs_validation";
            let bucket = match js.get_key_value(name).await {
                Ok(bucket) => bucket,
                Err(_) => {
                    js.create_key_value(async_nats::jetstream::kv::Config {
                        bucket: name.into(),
                        history: 1,
                        num_replicas: 1,
                        max_bytes: 16 * 1024 * 1024,
                        storage: async_nats::jetstream::stream::StorageType::File,
                        ..Default::default()
                    })
                    .await?
                }
            };
            let mut config = bucket.stream.cached_info().config.clone();
            if config.allow_direct {
                config.allow_direct = false;
                js.update_stream(config).await?;
            }
            let backend = NatsBackend::from_store_with_max_payload(
                js.get_key_value(name).await?,
                max_payload.min(512 * 1024),
            )?
            .with_lease_duration(LEASE);
            if role == "probe" {
                println!("NATS_MAX_PAYLOAD {max_payload}");
                let ctx = QueueCtx::new(tenant()?);
                let result = backend
                    .enqueue(
                        ctx,
                        dog_queue::JobMessage::new(
                            "capacity",
                            vec![0; 600_000],
                            "bytes",
                            "capacity",
                        ),
                    )
                    .await;
                match result {
                    Err(dog_queue::QueueError::InvalidConfig(_)) => {
                        println!("NATS_CAPACITY_REJECTED_BEFORE_WRITE")
                    }
                    Err(err) => bail!("JetStream admission exceeded hosted payload limit: {err}"),
                    Ok(_) => bail!("oversized payload unexpectedly admitted"),
                }
                return Ok(());
            }
            run(backend, role).await
        }
        #[cfg(feature = "rabbitmq")]
        "rabbitmq" => {
            let uri = secret("rabbitmq.uri")?;
            anyhow::ensure!(uri.starts_with("amqps://"), "hosted RabbitMQ requires TLS");
            let connection =
                lapin::Connection::connect(&uri, lapin::ConnectionProperties::default()).await?;
            let ledger: Arc<dyn JobLedger> = Arc::new(postgres().await?);
            let backend = RabbitMqBackend::new(
                connection.create_channel().await?,
                "dogrs-validation".into(),
                ledger,
            )
            .await?;
            if role == "probe" {
                return notification_probe(backend.notifications()).await;
            }
            run(backend, role).await
        }
        #[cfg(feature = "kafka")]
        "kafka" => {
            let mut config = rdkafka::ClientConfig::new();
            config
                .set("bootstrap.servers", secret("kafka.host")?)
                .set("security.protocol", "SSL")
                .set("ssl.ca.location", path("aiven-ca.pem")?.to_string_lossy())
                .set(
                    "ssl.certificate.location",
                    path("kafka-cert.pem")?.to_string_lossy(),
                )
                .set("ssl.key.location", path("kafka-key.pem")?.to_string_lossy())
                .set("enable.ssl.certificate.verification", "true")
                .set("ssl.endpoint.identification.algorithm", "https")
                .set("message.timeout.ms", "10000")
                .set("acks", "all");
            let producer = config.create::<rdkafka::producer::FutureProducer>()?;
            config.remove("acks");
            config.remove("message.timeout.ms");
            config
                .set("group.id", "dogrs-validation")
                .set("enable.auto.commit", "false")
                .set("auto.offset.reset", "earliest");
            let consumer = config.create::<rdkafka::consumer::StreamConsumer>()?;
            let backend = RdKafkaBackend::new(
                producer,
                consumer,
                "dogrs-validation".into(),
                Arc::new(postgres().await?),
            )?;
            if role == "probe" {
                return notification_probe(backend.notifications()).await;
            }
            run(backend, role).await
        }
        #[cfg(feature = "kafka-rust")]
        "kafka-rust" => {
            let mut roots = rustls::RootCertStore::empty();
            let ca = std::fs::read(path("aiven-ca.pem")?)?;
            for cert in rustls_pemfile::certs(&mut ca.as_slice()) {
                roots.add(cert?)?;
            }
            let pem = std::fs::read(path("kafka-cert.pem")?)?;
            let certs =
                rustls_pemfile::certs(&mut pem.as_slice()).collect::<std::io::Result<Vec<_>>>()?;
            let pem = std::fs::read(path("kafka-key.pem")?)?;
            let key = rustls_pemfile::private_key(&mut pem.as_slice())?
                .context("missing Kafka client key")?;
            let tls = rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_client_auth_cert(certs, key)?;
            let client = rskafka::client::ClientBuilder::new(vec![secret("kafka.host")?])
                .tls_config(Arc::new(tls))
                .build()
                .await?;
            let partition = client
                .partition_client(
                    "dogrs-validation",
                    0,
                    rskafka::client::partition::UnknownTopicHandling::Error,
                )
                .await?;
            let backend = dog_queue::backend::kafka::RsKafkaBackend::new(
                partition,
                Arc::new(postgres().await?),
            )
            .await?;
            if role == "probe" {
                return notification_probe(backend.notifications()).await;
            }
            run(backend, role).await
        }
        _ => bail!("unknown DOGRS_BACKEND"),
    }
}

/// Measures PostgreSQL transport latency across 100 ping tasks.
pub async fn network_probe() -> Result<()> {
    let db = Arc::new(postgres_client().await?);
    for bytes in [1024usize, 16384, 65536] {
        let payload = Arc::new(vec![42u8; bytes]);
        let mut tasks = tokio::task::JoinSet::new();
        let start = std::time::Instant::now();
        for index in 0..100u32 {
            tokio::time::sleep_until(tokio::time::Instant::from_std(
                start + Duration::from_millis(u64::from(index) * 100),
            ))
            .await;
            let db = db.clone();
            let payload = payload.clone();
            tasks.spawn(async move {
                let start = std::time::Instant::now();
                let row = db
                    .query_typed_one(
                        "SELECT octet_length($1::bytea)",
                        &[(&*payload, tokio_postgres::types::Type::BYTEA)],
                    )
                    .await?;
                anyhow::ensure!(
                    row.get::<_, i32>(0) == payload.len() as i32,
                    "unexpected transport echo"
                );
                Ok::<_, anyhow::Error>(start.elapsed().as_secs_f64() * 1000.0)
            });
        }
        let mut timings = vec![];
        while let Some(result) = tasks.join_next().await {
            timings.push(result??);
        }
        timings.sort_by(f64::total_cmp);
        println!(
            "{}",
            json!({"probe":"postgres_transport_only","payload_bytes":bytes,"requests":100,"offered_rps":10,"seconds":start.elapsed().as_secs_f64(),"p50_ms":timings[49],"p95_ms":timings[94]})
        );
    }
    Ok(())
}

// Cluster discovery can elect a leader before peer placement becomes available.
// Retry only that structured transient error; quota/auth/config failures stay fatal.
#[cfg(feature = "nats")]
pub async fn create_fixture_bucket(
    js: &async_nats::jetstream::Context,
    config: async_nats::jetstream::kv::Config,
) -> Result<async_nats::jetstream::kv::Store> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match js.create_key_value(config.clone()).await {
            Ok(bucket) => return Ok(bucket),
            Err(error) => {
                let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
                let mut placement_pending = false;
                while let Some(current) = cause {
                    if let Some(server) = current.downcast_ref::<async_nats::jetstream::Error>() {
                        placement_pending = server.error_code()
                            == async_nats::jetstream::ErrorCode::CLUSTER_NO_PEERS;
                    }
                    cause = current.source();
                }
                if !placement_pending || tokio::time::Instant::now() >= deadline {
                    return Err(error.into());
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn dispatch_backend<B: dog_queue::QueueBackend + 'static>(
    backend: B,
    role: &str,
) -> Result<()> {
    if role.starts_with("recovery-") {
        crate::recovery::run(backend, role).await
    } else {
        crate::capacity::run(backend).await
    }
}

/// Dispatches roles that execute against disposable local loopback backends (PostgreSQL, Redis, NATS).
pub async fn dispatch_local(role: &str) -> Result<()> {
    let backend = env("DOGRS_BACKEND")?;
    match backend.as_str() {
        "postgres" => {
            let uri = env("DOGRS_POSTGRES_URL")?;
            anyhow::ensure!(
                uri.contains("127.0.0.1") || uri.contains("localhost"),
                "local capacity requires loopback"
            );
            let mut options = dog_queue::backend::postgres::PostgresOptions {
                payload_storage: match std::env::var("DOGRS_PG_PAYLOAD_STORAGE").as_deref() {
                    Ok("external") => {
                        Some(dog_queue::backend::postgres::PostgresPayloadStorage::External)
                    }
                    Ok("extended") => {
                        Some(dog_queue::backend::postgres::PostgresPayloadStorage::Extended)
                    }
                    Err(std::env::VarError::NotPresent) => None,
                    _ => bail!("PostgreSQL payload storage must be external or extended"),
                },
                max_connections: std::env::var("DOGRS_PG_POOL_SIZE")
                    .unwrap_or_else(|_| "64".into())
                    .parse()?,
                enqueue_concurrency: std::env::var("DOGRS_PG_ENQUEUE_CONCURRENCY")
                    .ok()
                    .map(|n| n.parse())
                    .transpose()?,
                operation_timeout: Duration::from_secs(10),
                ..Default::default()
            };
            let shards: u32 = std::env::var("DOGRS_CAPACITY_SHARDS")
                .unwrap_or_else(|_| "1".into())
                .parse()?;
            anyhow::ensure!(
                (1..=16).contains(&shards),
                "PostgreSQL shards must be 1..=16"
            );
            if shards > 1 {
                anyhow::ensure!(
                    role == "capacity-local",
                    "sharded topology is for the capacity fixture"
                );
                anyhow::ensure!(
                    options.max_connections >= shards
                        && options.max_connections.is_multiple_of(shards),
                    "total pool size must divide evenly across shards"
                );
                options.max_connections /= shards;
                options.batch_concurrency = Some((4 / shards as usize).max(1));
                if let Some(cap) = options.enqueue_concurrency {
                    anyhow::ensure!(
                        cap >= shards && cap.is_multiple_of(shards),
                        "producer cap must divide evenly across shards"
                    );
                    options.enqueue_concurrency = Some(cap / shards);
                }
                let shard_urls: Vec<String> = std::env::var("DOGRS_PG_SHARD_URLS")
                    .ok()
                    .map(|value| serde_json::from_str(&value))
                    .transpose()?
                    .unwrap_or_else(|| vec![uri.clone(); shards as usize]);
                anyhow::ensure!(
                    shard_urls.len() == shards as usize,
                    "one PostgreSQL URL per fixed shard is required"
                );
                for url in &shard_urls {
                    let config: tokio_postgres::Config = url.parse()?;
                    anyhow::ensure!(
                        !config.get_hosts().is_empty()
                            && config.get_hosts().iter().all(|host| {
                                matches!(
                                    host,
                                    tokio_postgres::config::Host::Tcp(host)
                                        if host == "localhost"
                                            || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
                                )
                            }),
                        "capacity shard URLs must use loopback hosts"
                    );
                }
                let mut backends = Vec::new();
                for (shard, connection_string) in shard_urls.into_iter().enumerate() {
                    options.schema = Some(format!("dogrs_capacity_{shard}"));
                    backends.push(Arc::new(
                        dog_queue::backend::postgres::PostgresBackend::new_with_tls_options(
                            dog_queue::backend::postgres::PostgresConfig { connection_string },
                            tokio_postgres::NoTls,
                            options.clone(),
                        )
                        .await?,
                    ));
                }
                return dispatch_backend(
                    dog_queue::backend::sharded::ShardedBackend::new(backends)?,
                    role,
                )
                .await;
            }
            let backend = dog_queue::backend::postgres::PostgresBackend::new_with_tls_options(
                dog_queue::backend::postgres::PostgresConfig {
                    connection_string: uri,
                },
                tokio_postgres::NoTls,
                options,
            )
            .await?;
            let backend = backend.with_lease_duration(Duration::from_secs(
                if role == "capacity-local" { 300 } else { 2 },
            ));
            dispatch_backend(backend, role).await
        }
        #[cfg(feature = "redis")]
        "redis" => {
            let uri = env("DOGRS_REDIS_URL")?;
            anyhow::ensure!(
                uri.contains("127.0.0.1") || uri.contains("localhost"),
                "local capacity requires loopback"
            );
            let backend = dog_queue::backend::redis::RedisBackend::new(
                dog_queue::backend::redis::RedisConfig {
                    connection_string: uri,
                },
            )
            .await?
            .with_lease_duration(Duration::from_secs(
                if role == "capacity-local" { 300 } else { 2 },
            ));
            if std::env::var("DOGRS_REDIS_REQUIRE_AOF").as_deref() == Ok("1") {
                backend.verify_persistence().await?;
            }
            dispatch_backend(backend, role).await
        }
        #[cfg(feature = "nats")]
        "nats" => {
            let uri = env("DOGRS_NATS_URL")?;
            anyhow::ensure!(
                uri.contains("127.0.0.1") || uri.contains("localhost"),
                "local recovery requires loopback"
            );
            let name = env("DOGRS_NATS_BUCKET")?;
            let client = async_nats::connect(uri.split(',').collect::<Vec<_>>()).await?;
            let js = async_nats::jetstream::new(client);
            let shards: usize = std::env::var("DOGRS_CAPACITY_SHARDS")
                .unwrap_or_else(|_| "1".into())
                .parse()?;
            anyhow::ensure!((1..=32).contains(&shards), "shards must be 1–32");
            let connections =
                std::env::var("DOGRS_NATS_CONNECTIONS").unwrap_or_else(|_| "shared".into());
            anyhow::ensure!(
                matches!(connections.as_str(), "shared" | "per-shard"),
                "NATS connections must be shared or per-shard"
            );
            if shards > 1 {
                anyhow::ensure!(
                    role == "capacity-local",
                    "sharded topology is for the capacity fixture"
                );
                let mut backends = Vec::new();
                for shard in 0..shards {
                    let js = if connections == "per-shard" && shard > 0 {
                        async_nats::jetstream::new(
                            async_nats::connect(uri.split(',').collect::<Vec<_>>()).await?,
                        )
                    } else {
                        js.clone()
                    };
                    let name = format!("{name}_{shard}");
                    let bucket = create_fixture_bucket(
                        &js,
                        async_nats::jetstream::kv::Config {
                            bucket: name.clone(),
                            num_replicas: std::env::var("DOGRS_NATS_REPLICAS")
                                .unwrap_or_else(|_| "3".into())
                                .parse()?,
                            storage: async_nats::jetstream::stream::StorageType::File,
                            history: 1,
                            ..Default::default()
                        },
                    )
                    .await?;
                    let mut config = bucket.stream.cached_info().config.clone();
                    config.allow_direct = false;
                    config.allow_atomic_publish =
                        std::env::var("DOGRS_NATS_ATOMIC").as_deref() != Ok("0");
                    js.update_stream(config).await?;
                    backends.push(Arc::new(
                        dog_queue::backend::nats::NatsBackend::from_context(
                            js.clone(),
                            &name,
                            1024 * 1024,
                        )
                        .await?,
                    ));
                }
                return crate::capacity::run(dog_queue::backend::sharded::ShardedBackend::new(
                    backends,
                )?)
                .await;
            }
            let bucket = match js.get_key_value(&name).await {
                Ok(bucket) => bucket,
                Err(_) => {
                    create_fixture_bucket(
                        &js,
                        async_nats::jetstream::kv::Config {
                            bucket: name.clone(),
                            num_replicas: std::env::var("DOGRS_NATS_REPLICAS")
                                .unwrap_or_else(|_| "1".into())
                                .parse()?,
                            storage: async_nats::jetstream::stream::StorageType::File,
                            history: 1,
                            ..Default::default()
                        },
                    )
                    .await?
                }
            };
            let mut config = bucket.stream.cached_info().config.clone();
            config.allow_direct = false;
            config.allow_atomic_publish =
                std::env::var("DOGRS_NATS_ATOMIC").as_deref() != Ok("0");
            js.update_stream(config).await?;
            let backend = dog_queue::backend::nats::NatsBackend::from_context(
                js.clone(),
                &name,
                1024 * 1024,
            )
            .await?
            .with_lease_duration(Duration::from_secs(
                if role == "capacity-local" { 300 } else { 2 },
            ));
            dispatch_backend(backend, role).await
        }
        _ => bail!("unsupported local capacity backend"),
    }
}

