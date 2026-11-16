# Dependency security and preserved compatibility

Verified 16 November 2026. The workspace uses patched registry releases for the
reported vulnerabilities and unsoundness findings. CI rejects vulnerabilities,
unsoundness findings and yanked releases; no advisory is ignored. The AWS vendor
fork is retired in favor of aws-sigv4 1.6.5, retaining the independent signing
regressions in tests/aws-signing. See aws-sigv4a-security.md.

The 0.3 maintenance branch's authenticated TypeDB isolation, native policy error
handling, transaction cleanup, request deadlines and identity safeguards are
merged with the 0.4 framework and calendar work. Framework package versions and
0.4 dependencies are retained. TypeQL's newer parser represents writes with
WritePattern; the compiler validates that AST directly and continues to reject
unsupported proposals. The TypeDB driver/protocol remains 3.13-compatible.

RabbitMQ uses the maintained lapin 4.12 client. AMQP queue names and public
confirmation types follow its API; mandatory publication, publisher confirms,
durable queues, bounded prefetch, acknowledgement and shutdown ordering remain.
The job ledger and serialized job format are unchanged. The unused optional
bincode dependency had no codec implementation or persisted format and is
removed; it is no longer an advertised Cargo feature. Callers that enabled that
empty feature should remove it from their feature list.

The hosted example reads PEM certificates and keys through maintained
rustls-pki-types APIs while retaining verified TLS, CA roots and client auth.
Iroh blobs uses 0.105 with the corresponding Bao validation and streaming APIs.
Compatibility checks cover existing serialized payloads and filesystem stores.

## Removal of the four unmaintained dependencies

The resolved workspace graph now contains none of rustls-pemfile,
proc-macro-error, atomic-polyfill or paste. CI rejects unmaintained findings as
well as vulnerabilities, unsoundness and yanked releases, without ignores or
platform filters.

Released upstream versions do not yet provide this entire combination. Root
Cargo patches pin explicit, reviewable source commits, not renamed obsolete
packages. JITPOMI owns the following compatibility branches and their maintenance:

- TypeDB driver 3.13.6: `Jitpomi/typedb-driver` at `548a885`, based on the exact
  upstream release. Tonic/Prost move to 0.14; the channel uses its public Body
  and interceptor response-body types. Authentication injection, CA roots,
  hostname verification and transactions remain. The unused async-std normal
  dependency is removed, matching the prior registry package. The C client's
  sync feature stays explicit at its own dependency rather than forcing it onto
  the asynchronous Rust test harness.
- TypeDB protocol 3.13.4: `Jitpomi/typedb-protocol` at `2644e0d`, selected by the
  driver. Tonic/Prost generation uses unchanged protobuf definitions and a
  registry-provided protoc. TLS tests reject untrusted roots and wrong names.
- Postcard 1.1.3: `Jitpomi/postcard` at `d3df0e1`. Fixed-capacity serialization
  uses heapless 0.8 and portable-atomic, preserving its wire encoding, feature
  names and overflow behavior. Public heapless return values are now 0.8 types;
  callers directly naming the older type must update their dependency.
- Netlink core 0.8.2: `Jitpomi/netlink-packet-core` at `8d50a3e`. Its implementation
  and exported macros use maintained pastey. Upstream parser/packet tests and
  updated route examples exercise the same public and wire behavior on Linux.

Iroh blobs moves to registry 0.105 and Bao tree uses upstream's immutable
`d50f2b2` commit, which removes unused procedural macro support while retaining
validation, streaming and filesystem features. The coroutine implementation
remains; only the unused generator macro feature is removed. Blob hashes,
request payload limits and stored payload encoding stay unchanged.

All forks retain upstream history, source and licenses, and document their base,
changes, tests and replacement policy in JITPOMI-MAINTENANCE.md. Replace a source
pin only when the corresponding maintained upstream registry release passes the
same compatibility checks. Cargo root patches are application/workspace scoped;
they do not propagate to downstream crates.io consumers. These changes do not
claim to fix already-published DogRS packages or publish a new release. A future
release must use maintained registry dependencies or explicitly published
maintained integration packages before removing the root pins.

## Verification

Run workspace, documentation, transport-feature and warning-denied all-feature
checks; the TypeDB policy/error suite; real durable queue/broker tests; S3/blob
and signer tests; realtime and recovery suites. Use isolated test databases,
queues and stores. Keep source data and checkpoints intact. Business separately
replays its retained Stripe and Mercury plugin fixtures and native authorization
checks. Neither repository automatically rewrites a live data store on startup.
