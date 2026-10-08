# JetStream 500 jobs/second operating baseline

Three fresh-fixture runs passed at 500 jobs/second across 100 tenants, with unique
64 KiB payloads. This is a measured baseline for the configuration below, not a
universal adapter limit, an hours-long soak certification, or a fix for the
unmet 1,000/s stress target. The queue adapter is unchanged from `e77f606`.

## Configuration and results

Each run offered five jobs/second per tenant for 120 seconds, with five seconds
allowed for draining. All retained R3, file storage and always-fsync on three
colocated NATS 2.15.0 Docker nodes, 16 shards, one shared client connection, four
workers and 32 in-flight offers per tenant. The existing atomic writer and its
default batching/concurrency settings were unchanged. Diagnostics were disabled.
The host was an Apple M1 Max with 64 GiB RAM; this is not a separate-disk or
independent-failure-domain deployment. Each fixture started empty and was removed
after verification.

| Run | Accepted | Completed by deadline | Verified | Elapsed | Enqueue p95 |
|---:|---:|---:|---:|---:|---:|
| 1 | 60,000 | 60,000 | 60,000 | 120.224 s | 335.5 ms |
| 2 | 60,000 | 60,000 | 60,000 | 120.211 s | 275.5 ms |
| 3 | 60,000 | 60,000 | 60,000 | 120.207 s | 1029.0 ms |

Every run had zero operation errors, overload rejections and late offers. Each
verified all 60,000 terminal jobs and payload integrity. Total: 180,000 jobs.
Enqueue p95 varied substantially; a throughput pass does not establish a strict
latency guarantee. These are three separate two-minute workloads, not one
continuous six-minute workload or a growing-history soak.

## Reproduce and interpret

Build `hosted-system` in release mode with `redis,nats` features, then follow the
[operating-rate command](../dog-examples/hosted-system/README.md#separate-operating-rate-and-stress-measurements).
Use `--rate 5 --seconds 120 --bytes 65536` with four workers and 16 shards.
The runner always uses 100 tenants. The reported rate is per tenant.

The harness now accepts an explicit rate instead of hard-coding ten. Its default
remains ten, preserving the 1,000/s stress benchmark, including pull-request runs.
Manual Provider capacity runs expose both rates and a 120-second duration.
Admission, completion, integrity, zero-error and drain checks remain unchanged;
lowering the offered rate does not relabel historical failures.

No new adapter redesign is justified by this result. The split-storage candidate
was [rejected](jetstream-split-storage-experiment.md), and the outstanding 1,000/s
bottleneck is not proven to be exclusively infrastructure. Deployments should
validate their own hardware, latency budget, longer retained histories and
failure scenarios before relying on this rate. Recovery evidence remains
separate; these three runs injected no failures.

Raw reports and runner are preserved under `dogrs-operating-500` in the task
workspace. Release binary SHA-256: `0230b8b59d2a3cfa0933cabec6c067c276bb91e4307ecd30b46254af91be7ab3`.
No paid resources were created.
