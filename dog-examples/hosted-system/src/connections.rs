use super::*;
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
use std::path::PathBuf;
use tokio_postgres_rustls::MakeRustlsConnect;

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
                            vec![0; 300_000],
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
