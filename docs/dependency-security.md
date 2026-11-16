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
Iroh retains its compatible 0.103 series at the non-yanked 0.103.1 patch; existing
blob hashes, payloads, storage types and peer protocol are preserved.

## Remaining upstream informational warnings

These are unmaintained-package warnings, not cleared or ignored findings:

- rustls-pemfile: TypeDB driver -> tonic 0.12. The latest TypeDB driver also
  requests that transport series. There is no patched pemfile release.
- proc-macro-error: iroh-blobs/bao-tree -> genawaiter macros. The latest upstream
  iroh-blobs still reaches this chain; removing blob support would lose work.
- atomic-polyfill: postcard's heapless support, including Iroh dependencies.
- paste: Iroh's Linux netwatch/netlink dependency chain.

Removing these remaining warnings requires upstream maintained replacements or
an explicitly validated transport/storage migration. No local fork, TLS bypass,
feature disabling, target filtering or audit ignore is used to report clearance.
All warnings remain visible in the CI audit output.

## Verification

Run workspace, documentation, transport-feature and warning-denied all-feature
checks; the TypeDB policy/error suite; real durable queue/broker tests; S3/blob
and signer tests; realtime and recovery suites. Use isolated test databases,
queues and stores. Keep source data and checkpoints intact. Business separately
replays its retained Stripe and Mercury plugin fixtures and native authorization
checks. Neither repository automatically rewrites a live data store on startup.
