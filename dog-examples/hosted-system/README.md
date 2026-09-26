# Hosted DogRS system acceptance

This is a synthetic payment-recording application, with a DogRS HTTP service,
typed `QueueAdapter` jobs, independently started worker processes and PostgreSQL
business records. It never calls a payment provider, sends email or uses customer
data. The API binds to loopback and requires a randomly generated bearer token.
The database and broker connections cross the public network with certificate
and hostname verification enabled.

The controller kills a worker after its business effect commits and before DogRS
can acknowledge the job. It restarts the API and competing workers, then checks
lease recovery and the application's unique-key deduplication. This is at-least-once
delivery with idempotent effects, not a claim that a broker guarantees exactly once.

## Run

```sh
cargo build -p hosted-system --all-features --locked
export DOGRS_SYSTEM_BINARY="$PWD/target/debug/hosted-system"
export DOGRS_SECRETS_DIR=/absolute/private/directory
export DOGRS_REPORT_DIR=/absolute/results/directory
python3 dog-examples/hosted-system/run_system.py postgres
```

Repeat with `nats`, `redis`, `rabbitmq`, `kafka` (librdkafka) `kafka-rust` (rskafka), and `sqs`. Run them sequentially: the test
API uses port 38171. Each run creates its own `dogrs-test-…` tenant, submits 46
synthetic jobs and kills only processes that the controller starts. Failed runs
remain in the results directory as evidence; later success does not erase them.

The secret directory contains these files (never commit it):

| File | Purpose |
|---|---|
| `aws.json` | Optional queue-only AWS credentials: `access_key_id`, `secret_access_key`, optional `session_token` |
| `postgres.uri` | Dedicated Aiven PostgreSQL URI requiring TLS |
| `aiven-ca.pem` | Aiven project CA, used with certificate/hostname verification |
| `redis.uri` | `rediss://` URI for the dedicated Valkey test service |
| `rabbitmq.uri` | CloudAMQP `amqps://` URI |
| `nats.creds` | Dedicated Synadia test account credentials |
| `kafka.host` | Aiven Kafka mutual-TLS endpoint, including port |
| `kafka-cert.pem`, `kafka-key.pem` | Kafka client certificate and private key |

Provision the Kafka topic `dogrs-validation` first. RabbitMQ creates only its
`dogrs-validation` queue. NATS creates a dedicated `dogrs_validation` KV bucket
with a 16 MiB storage limit, one replica, no TTL and leader reads. The hosted
example explicitly supplies its verified 512 KiB account payload limit; the
NATS server INFO limit can be higher than the account limit. Adjust provider
addresses and account limits when using another environment.

`hosted-system init` creates only the two `dogrs_validation_*` business tables.
DogRS initializes its queue table. The test uses a short 8-second backend lease,
2-second heartbeat, two workers in each of two processes, and an 18-second job.
Do not copy these tight test deadlines into a deployment without measuring latency.
`QueueConfig.lease_duration` and the backend lease setting must agree.

## Checks and evidence

- Reject requests without the test bearer token.
- Deduplicate an active job submitted through HTTP.
- Refuse another tenant access to the same job ID.
- Cancel a queued job without executing its business effect.
- Recover after API and worker death, including a crash after committing an effect.
- Retry transient failures and retain permanent failure status.
- Keep a long-running job leased through delayed backend round trips.
- Complete a 40-job batch submitted by four concurrent clients, using both worker processes.
- Preserve terminal results across complete process restart.
- For RabbitMQ/Kafka/SQS, independently verify a real broker round trip. Ledger polling
  must not hide broken TLS, authentication or broker permissions.

`hosted-system probe` with `DOGRS_BACKEND=nats` checks admission against the hosted
payload limit. With `DOGRS_BACKEND=redis`, it prints the service's AOF and snapshot
status. JSON results include elapsed throughput and enqueue latency. These are
small acceptance workloads, not capacity certifications or comparative benchmarks.

The `Hosted system acceptance` workflow repeats the application on a standard
GitHub Linux runner. Credentials live in the restricted `dogrs-hosted-validation`
environment and are never made available to arbitrary pull-request code. The
workflow uses manual dispatch, not a schedule or automatic push trigger. It creates no
cloud services and uses only the pre-provisioned free services.

## Findings and limits

The first hosted PostgreSQL run exposed heartbeat drift: the worker slept an
interval, waited for the backend, then extended by only the interval. Network time
accumulated until the lease expired. The fix accounts for elapsed time, and the
latency regression fails on the old code. Status reads also no longer rewrite
entire tenant records or take unnecessary PostgreSQL write locks.

Synadia requires a stream storage bound and applies account payload limits lower
than the advertised server limit. `NatsBackend::from_store_with_max_payload`
reserves framing and completion space before admitting work.

Aiven Valkey reports `aof_enabled:0`; Aiven does not support AOF. Its successful
application-process recovery is **not** evidence of lossless recovery after a
provider server crash. Use a suitably persistent/replicated deployment or another
ledger for that requirement. PostgreSQL, Redis and JetStream still serialize tenant
state; a successful small test does not remove that throughput limit.

These tests do not certify provider failover, backup restore, long outages,
clock skew, large payloads, sustained load, cloud IAM policies or an always-on
production deployment. SQS is covered by the hosted Linux workflow when its
restricted test credential is supplied. Google Pub/Sub is exercised separately
inside Google Cloud Shell in the same region as its message storage. Their
emulator coverage remains separate evidence.

## Cleanup

The controller terminates its own processes. Dedicated free services remain so
runs can be repeated; remove them explicitly through the provider consoles when
finished. Purge terminal test history using the ledger's retention API and delete
only matching test-tenant rows from the business tables. Do not delete another
tenant's records. Never put these credentials in public logs or artifacts.

SQS uses the private `dogrs-validation` queue in the dedicated AWS validation account, region `us-east-2`. Its IAM test identity can only send, receive and delete messages on that queue over TLS. Disable the credential after validation. The workflow includes SQS only when `aws.json` is supplied. Removing that entry after deactivation keeps future runs limited to available credentials.

For `pubsub`, run inside a Google Cloud environment in `us-west1`, using existing Application Default Credentials and `DOGRS_GCP_PROJECT`. Pre-create `dogrs-validation` topic and subscription with message storage restricted to `us-west1`, no topic retention, and subscription retention at most one day. The client uses the regional HTTPS endpoint. This transport is excluded from the external GitHub runner to avoid Pub/Sub internet delivery charges.
