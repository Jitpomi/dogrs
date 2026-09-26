# Redis and JetStream per-job storage

Both adapters are optional. Neither requires PostgreSQL. Applications can also
supply their own `JobLedger` implementation for broker adapters.

## Redis

Concurrent claimers may exhaust a bounded scan by losing compare-and-swap races
to other workers. Such a dequeue returns `None` because it acquired no lease;
callers should continue polling. Backend connectivity errors and failures of
other state-changing operations still propagate as errors.

V2 stores metadata and binary payloads in separate per-tenant hashes. Ready,
delayed, leased and terminal jobs have separate sorted-set indexes. Lua scripts
atomically compare the selected record, change its state, and update its indexes
and idempotency entry. Unrelated jobs no longer contend on one tenant document.
Redis TIME supplies transition timestamps; acknowledgement scripts check expiry
again at commit. Terminal history does not enter the dequeue selection path.
All atomic record/index keys share a tenant hash tag. The current connection
constructor targets a single Redis endpoint, not a Redis Cluster router.

`new` retains compatibility/development behavior: it does not certify persistence.
Use `new_with_durability(config, RedisDurability::RequireAofAlways)` to fail startup
unless AOF is enabled, appendfsync is `always`, and maxmemory-policy is `noeviction`.
Reconnect cycles are bounded below the 30-second operation deadline (up to four
five-second connection attempts with bounded backoff). This avoids a dead
connection keeping recovery pending beyond an operation budget.
The durability guard needs permission for INFO and CONFIG GET. `verify_persistence()` can repeat
those checks after configuration changes. It does not verify replica catch-up,
failover policy, disk reliability, or a managed provider's guarantees. A service
with AOF disabled cannot pass this guard.

Retention removes up to 1,000 terminal records and their payloads per call; repeat
`purge_terminal_before` until it returns zero. Active records are never purged.

## JetStream

A revision-fenced active cell contains one record per idempotency scope (queue,
job type and key), or one independent record for an unkeyed job. Payloads are
immutable separate binary values, written before enqueue becomes visible.
Completion commits the terminal record in its current cell and removes it from
the in-memory discovery index. It does not wait for a second archive write and
cleanup write. Before an idempotency scope is reused, enqueue archives the previous
terminal record and then conditionally replaces the cell. Old tokens never
authorize a replacement job. Retention covers both current terminal cells and
archived history, and leaves tombstones that prevent delayed archival from
recreating intentionally purged records. Purge removes at most 1,000 jobs per call;
repeat it until no eligible records remain.

NATS submissions have a default concurrency bound of 16 per backend instance.
`with_enqueue_concurrency` accepts 1–4096 permits. Claims, completions and lease
updates do not acquire these producer permits.

An ordered, replayable watch supplies discovery hints without transferring
payloads or completed history on every poll. Claims always re-read and CAS the
actual cell. Discovery metadata is decoded once per revision and reused across
polls; malformed metadata remains an error. Each tenant has its own discovery
index, and dequeue selects the next job in one pass without sorting/cloning all
candidates or scanning other tenants. Watch end/errors trigger reconstruction. The watch is not ownership
authority. Dequeue skips stale or not-yet-visible hints and considers other jobs;
explicit ID reads still report missing records. Results remain limited to 4 KiB; admission reserves 8 KiB of metadata
space for status updates and checks binary payloads against the account limit.
Maintain account/bucket headroom: per-message checks cannot reserve total provider
quota. An enqueue whose outcome is unknown can leave an unreferenced payload;
do not delete payloads merely because a timed-out enqueue returned an error.

Use file storage, no expiry/eviction, discard-new, and leader reads. For clustered
deployments, `from_replicated_store(bucket, max_payload, min_replicas)` requires
at least three replicas. Provision server fsync policy and failure-domain placement
separately; stream replica count alone cannot verify those properties. Native
JetStream lease deadlines still require synchronized application clocks. CAS
fences replaced owners, but this does not certify arbitrary wall-clock jumps.

## Upgrade boundary

These are new storage layouts in the unreleased 0.2 work. An existing v1 tenant
is rejected explicitly rather than silently starting a second ledger. Keep the
previous binary available to drain/export it, stop legacy writers, and verify an
offline import into a fresh tenant/bucket before switching. There is no automatic
Redis/JetStream v1 migration in this change, and no existing hosted tenant is
rewritten by these constructors. PostgreSQL has its own explicit, fenced
migration described in [POSTGRES.md](POSTGRES.md).

## Recovery evidence and limits

`run_recovery.py` starts disposable real servers, admits 200 jobs with 64 KiB
payloads, completes 50, leaves 50 in flight, and queues 100. It kills the server
process and resumes the same backend instance. The three-node JetStream variant
kills the actual stream leader and keeps it down. Verification checks every
acknowledged payload, rejects expired owners, and completes the remaining jobs.

PostgreSQL default WAL durability, Redis AOF/always/noeviction, and JetStream
three-replica/file/sync-always configurations passed this test. This is controlled
process-crash/failover evidence, not a proof against complete disk loss, correlated
regional outages, clock jumps, or undocumented managed-service behavior.

Persistence references: [Redis persistence](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/)
and [JetStream durability](https://docs.nats.io/nats-concepts/jetstream).

## Horizontal scaling without a fixed stack

`backend::sharded::ShardedBackend` routes each tenant over caller-provisioned
backends using documented FNV-1a-64/UTF-8 modulo a fixed shard count. It works with
PostgreSQL, Redis, JetStream, or custom backends, including trait objects. Ownership,
idempotency, status reads and events use the same tenant route; maintenance visits
all shards. Only shards that implement `JobLedger` produce a durable sharded ledger.
The advertised capabilities are the intersection of all shards.

The ordered shard list is persistent configuration. Changing its size or order
requires an offline tenant migration; never use it as a live autoscaling switch.
Sharding creates no hosted resources. Provision independent stores yourself, and
benchmark their actual hardware, replica count and fsync settings. A routing
wrapper does not turn one physical disk into independent failure domains.

## Extended recovery fixture

`run_recovery.py redis --restore --outage-seconds 300 --report-dir /absolute/results`
keeps the same client alive during five minutes of failed connectivity, copies
crash-persisted AOF files, removes the original container, and restores those files
into a fresh container. The observed run preserved all 200 acknowledged 64 KiB
jobs (98 failed connectivity probes), rejected 50 expired owners, and finished the
150 remaining jobs. This verifies restoration of the latest crash image, not
zero-loss recovery from an older backup. PostgreSQL supports the same cold-file
fixture; JetStream uses its separate actual-leader failure test.

`--race` runs concurrent metadata reads against claims/completions/retirement on
640 full-size jobs. `DOGRS_NATS_IMAGE` selects the disposable server image; the
compatibility fixture defaults to NATS 2.11, while the capacity workflow pins
2.15.0 with three replicas and disk synchronization enabled.


Redis successful claims return the binary payload from the same atomic CAS script
response; losing claims return no payload. This removes a separate payload GET
without changing ownership checks or the stored format.

JetStream completion may apply its transition to cached metadata, but success
still requires CAS of that exact server revision. A stale validation result or a
revision conflict falls back to an authoritative read, preserving remote
cancellation and heartbeat behavior. Claims and explicit reads continue to read
the server. This is an optional-backend optimization, not a portable API change.
