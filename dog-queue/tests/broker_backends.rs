#![cfg(feature = "postgres")]
use dog_queue::{
    backend::{
        broker::{BrokerBackend, JobLedger, Notifications},
        postgres::{PostgresBackend, PostgresConfig},
    },
    JobMessage, JobStatus, QueueBackend, QueueCtx, QueueError, QueueResult,
};
use std::{sync::Arc, time::Duration};
mod common;
async fn ledger() -> Arc<dyn JobLedger> {
    Arc::new(
        PostgresBackend::new(PostgresConfig {
            connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        })
        .await
        .unwrap(),
    )
}
struct Offline;
#[async_trait::async_trait]
impl Notifications for Offline {
    async fn publish(&self) -> QueueResult<()> {
        Err(QueueError::Internal("simulated outage".into()))
    }
    async fn receive(&self) -> QueueResult<bool> {
        Err(QueueError::Internal("simulated outage".into()))
    }
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn broker_outage_does_not_lose_committed_jobs() {
    let a = Arc::new(BrokerBackend::with_ledger(Offline, ledger().await));
    let b = Arc::new(BrokerBackend::with_ledger(Offline, ledger().await));
    let short_ledger = PostgresBackend::new(PostgresConfig {
        connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
    })
    .await
    .unwrap()
    .with_lease_duration(Duration::from_millis(50));
    let short = Arc::new(BrokerBackend::with_ledger(Offline, Arc::new(short_ledger)));
    common::contract(a.clone(), b, short).await;
    assert!(a.notification_failures().0 > 0);
    assert!(a.notification_failures().1 > 0);
    let tenant = QueueCtx::new(format!("restart-{}", uuid::Uuid::new_v4()));
    let id = a
        .enqueue(
            tenant.clone(),
            JobMessage::new("restart", vec![], "json", "q"),
        )
        .await
        .unwrap();
    drop(a);
    let restarted = BrokerBackend::with_ledger(Offline, ledger().await);
    let job = restarted
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.record.job_id, id);
    restarted
        .ack_complete(tenant.clone(), id.clone(), job.lease_token, None)
        .await
        .unwrap();
    assert!(matches!(
        restarted.get_status(tenant, id).await.unwrap(),
        JobStatus::Completed { .. }
    ));
}

#[cfg(feature = "rabbitmq-lapin")]
#[tokio::test]
#[ignore = "requires disposable RabbitMQ and PostgreSQL"]
async fn rabbitmq_notifications_and_job_completion() {
    use dog_queue::backend::rabbitmq::RabbitMqBackend;
    let connection = lapin::Connection::connect(
        &std::env::var("DOGRS_RABBITMQ_URL").unwrap(),
        lapin::ConnectionProperties::default(),
    )
    .await
    .unwrap();
    let channel = connection.create_channel().await.unwrap();
    let queue = format!("dogrs-test-{}", uuid::Uuid::new_v4());
    let mut backend = RabbitMqBackend::new(channel.clone(), queue.clone(), ledger().await)
        .await
        .unwrap();
    backend.notifications().publish().await.unwrap();
    receive_one(backend.notifications()).await;
    let state = channel
        .queue_declare(
            &queue,
            lapin::options::QueueDeclareOptions {
                passive: true,
                ..Default::default()
            },
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(state.message_count(), 0);
    let tenant = QueueCtx::new(queue.clone());
    let id = backend
        .enqueue(
            tenant.clone(),
            JobMessage::new("test", vec![42], "json", "q"),
        )
        .await
        .unwrap();
    let job = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.record.job_id, id);
    assert!(matches!(
        backend
            .get_status(tenant.clone(), id.clone())
            .await
            .unwrap(),
        JobStatus::Processing { .. }
    ));
    backend
        .ack_complete(tenant.clone(), id.clone(), job.lease_token, None)
        .await
        .unwrap();
    assert!(matches!(
        backend.get_status(tenant, id).await.unwrap(),
        JobStatus::Completed { .. }
    ));
    backend.shutdown_notifications().await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let state = channel
                .queue_declare(
                    &queue,
                    lapin::options::QueueDeclareOptions {
                        passive: true,
                        ..Default::default()
                    },
                    Default::default(),
                )
                .await
                .unwrap();
            if state.consumer_count() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("shutdown cancels the persistent RabbitMQ consumer");
    channel
        .queue_delete(&queue, Default::default())
        .await
        .unwrap();
    connection.close(200, "test complete").await.unwrap();
}

#[cfg(feature = "aws-sqs")]
#[tokio::test]
#[ignore = "requires disposable SQS emulator and PostgreSQL"]
async fn sqs_notifications_and_job_completion() {
    use aws_sdk_sqs::{
        config::{BehaviorVersion, Credentials, Region},
        Client, Config,
    };
    use dog_queue::backend::aws_sqs::AwsSqsBackend;
    let config = Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new(
            "local-test",
            "local-test",
            None,
            None,
            "test",
        ))
        .endpoint_url(std::env::var("DOGRS_SQS_URL").unwrap())
        .build();
    let client = Client::from_conf(config);
    let queue = format!("dogrs-test-{}", uuid::Uuid::new_v4());
    let response = client
        .create_queue()
        .queue_name(&queue)
        .send()
        .await
        .unwrap();
    let url = response.queue_url().unwrap().to_string();
    let backend = AwsSqsBackend::new(client.clone(), url.clone(), ledger().await).unwrap();
    backend.notifications().publish().await.unwrap();
    receive_one(backend.notifications()).await;
    let tenant = QueueCtx::new(queue);
    let id = backend
        .enqueue(
            tenant.clone(),
            JobMessage::new("test", vec![42], "json", "q"),
        )
        .await
        .unwrap();
    let job = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.record.job_id, id);
    assert!(matches!(
        backend
            .get_status(tenant.clone(), id.clone())
            .await
            .unwrap(),
        JobStatus::Processing { .. }
    ));
    backend
        .ack_complete(tenant.clone(), id.clone(), job.lease_token, None)
        .await
        .unwrap();
    assert!(matches!(
        backend.get_status(tenant, id).await.unwrap(),
        JobStatus::Completed { .. }
    ));
    client.delete_queue().queue_url(url).send().await.unwrap();
}

#[cfg(any(
    feature = "rabbitmq-lapin",
    feature = "aws-sqs",
    feature = "kafka-rdkafka",
    feature = "kafka-rskafka",
    feature = "gcp-pubsub"
))]
async fn receive_one(notifications: &impl Notifications) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if notifications.receive().await.unwrap() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Published notification must reach a consumer");
}

#[cfg(feature = "kafka-rdkafka")]
#[tokio::test]
#[ignore = "requires disposable Kafka and PostgreSQL"]
async fn rdkafka_notifications() {
    use dog_queue::backend::kafka::RdKafkaBackend;
    let brokers = std::env::var("DOGRS_KAFKA_BROKERS").unwrap();
    let topic = std::env::var("DOGRS_KAFKA_TOPIC").unwrap();
    let producer = rdkafka::ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set("acks", "all")
        .create()
        .unwrap();
    let consumer = rdkafka::ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set("group.id", format!("dogrs-test-{}", uuid::Uuid::new_v4()))
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .create()
        .unwrap();
    let backend = RdKafkaBackend::new(producer, consumer, topic, ledger().await).unwrap();
    backend.notifications().publish().await.unwrap();
    receive_one(backend.notifications()).await;
}

#[cfg(feature = "kafka-rskafka")]
#[tokio::test]
#[ignore = "requires disposable Kafka and PostgreSQL"]
async fn rskafka_notifications() {
    use dog_queue::backend::kafka::RsKafkaBackend;
    let client =
        rskafka::client::ClientBuilder::new(vec![std::env::var("DOGRS_KAFKA_BROKERS").unwrap()])
            .build()
            .await
            .unwrap();
    let partition = client
        .partition_client(
            std::env::var("DOGRS_KAFKA_TOPIC").unwrap(),
            0,
            rskafka::client::partition::UnknownTopicHandling::Error,
        )
        .await
        .unwrap();
    let backend = RsKafkaBackend::new(partition, ledger().await)
        .await
        .unwrap();
    backend.notifications().publish().await.unwrap();
    receive_one(backend.notifications()).await;
}

#[cfg(feature = "gcp-pubsub")]
#[tokio::test]
#[ignore = "requires disposable Pub/Sub emulator and PostgreSQL"]
async fn pubsub_notifications() {
    use dog_queue::backend::gcp_pubsub::GcpPubSubBackend;
    use google_cloud_auth::credentials::anonymous::Builder as Anonymous;
    use google_cloud_pubsub::client::{Publisher, Subscriber, SubscriptionAdmin, TopicAdmin};
    let endpoint = std::env::var("DOGRS_PUBSUB_ENDPOINT").unwrap();
    assert!(
        endpoint.starts_with("http://127.0.0.1:"),
        "This test must use a local emulator"
    );
    let topic = format!("projects/dogrs-test/topics/test-{}", uuid::Uuid::new_v4());
    let subscription = format!(
        "projects/dogrs-test/subscriptions/test-{}",
        uuid::Uuid::new_v4()
    );
    let topics = TopicAdmin::builder()
        .with_endpoint(&endpoint)
        .with_credentials(Anonymous::new().build())
        .build()
        .await
        .unwrap();
    let subscriptions = SubscriptionAdmin::builder()
        .with_endpoint(&endpoint)
        .with_credentials(Anonymous::new().build())
        .build()
        .await
        .unwrap();
    topics.create_topic().set_name(&topic).send().await.unwrap();
    subscriptions
        .create_subscription()
        .set_name(&subscription)
        .set_topic(&topic)
        .send()
        .await
        .unwrap();
    let publisher = Publisher::builder(&topic)
        .with_endpoint(&endpoint)
        .with_credentials(Anonymous::new().build())
        .build()
        .await
        .unwrap();
    let subscriber = Subscriber::builder()
        .with_endpoint(&endpoint)
        .with_credentials(Anonymous::new().build())
        .build()
        .await
        .unwrap();
    let backend =
        GcpPubSubBackend::new(publisher, subscriber, subscription.clone(), ledger().await).unwrap();
    backend.notifications().publish().await.unwrap();
    receive_one(backend.notifications()).await;
    drop(backend);
    subscriptions
        .delete_subscription()
        .set_subscription(subscription)
        .send()
        .await
        .unwrap();
    topics.delete_topic().set_topic(topic).send().await.unwrap();
}
