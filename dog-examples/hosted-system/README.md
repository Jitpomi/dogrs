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
ledger for that requirement. Redis and JetStream still serialize tenant state. PostgreSQL v2 uses per-job rows
and binary payloads; see its explicit offline migration requirements in
[POSTGRES.md](../../dog-queue/POSTGRES.md). A small hosted run does not establish
aggregate production capacity.

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

## Production gates

The agreed launch workload and outstanding release gates are tracked in
[production readiness](../../docs/production-readiness.md). Run
`run_capacity.py` with the same binary/secret/report environment to test an
open-loop arrival rate and increasing payload sizes. The first hosted 10-job/s
single-tenant gate failed. Passing this acceptance suite does not override that
failed capacity gate.

`DOGRS_TEST_WORKERS` selects workers per process (default 2, maximum 32). Capacity
runs with 8 workers per process keep the PostgreSQL pool at four connections per
process. `get_snapshot` polls execution metadata without downloading job payloads.
Before running against a v1 test ledger, take a backup, stop old test processes,
and run `DOGRS_BACKEND=postgres hosted-system migrate` with the private secrets
directory configured. The normal commands refuse an unmigrated nonempty ledger.

For production-like compilation use `cargo build --release -p hosted-system` and
set `DOGRS_SYSTEM_BINARY` to the release executable. The `network-probe` role
separately measures PostgreSQL transport with 100 binary parameters per payload
size at 10 requests/second, without queue operations or persistent writes.

### Separate operating-rate and stress measurements

The aggregate `run_recovery.py --capacity` workload accepts `--rate` in jobs per
second **per tenant** (1–10). It always uses 100 tenants. The default is nine,
so ordinary runs target 900 jobs/second. Explicit `--rate 10` retains the 1,000/s
stress benchmark. Five tests the measured 500/s
operating target without changing payload integrity, durability, zero-error or
five-second drain requirements:

```sh
DOGRS_SYSTEM_BINARY=target/release/hosted-system \
DOGRS_CAPACITY_WORKERS=4 DOGRS_CAPACITY_SHARDS=16 \
python3 dog-examples/hosted-system/run_recovery.py nats --capacity \
  --rate 5 --seconds 120 --bytes 65536 --report-dir operating-capacity
```

Repeat on fresh fixtures and inspect every verdict. A lower-rate pass never
reclassifies a failed 1,000/s run. The Provider capacity manual workflow exposes
all three rates and the 120-second duration; pull requests target 900/s. Attribution profiles retain their original fixed rate and reject a
non-default rate in the runner. Capacity results record the actual per-tenant rate.

### Compare diagnostics on one runner

The Provider capacity workflow's `queue-comparison` mode runs PostgreSQL, Redis or NATS four
times on the same runner, with fresh storage each time: diagnostics off, on, on,
off. Select 64 KiB payloads and 60 or 120 seconds. One release binary includes the
diagnostic feature throughout; the environment toggle changes between trials.
For NATS, diagnostic mode also enables the fixture's server-stack sampling.
This compares the entire diagnostic mode, not just one counter's overhead.

`compare_queue_diagnostics.py` saves every measurement and the binary hash in
`diagnostic-comparison.json`. Any missing measurement or failed trial fails the
comparison. Individual logs, environment snapshots and server diagnostics remain
in trial subdirectories. The workflow's summary scans these subdirectories.
Do not combine this mode with other fixture-comparison settings or profiling
experiments: use the ordinary atomic/shared-connection defaults. Same-runner
repeats control machine assignment but do not guarantee constant disk contention.
