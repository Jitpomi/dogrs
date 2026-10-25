# Temporary AWS SigV4a dependency patch

Source: the published `aws-sigv4` 1.6.3 crate from crates.io (Apache-2.0).
The source and AWS signing fixtures are unchanged. The sole functional change
is raising the direct `crypto-bigint` requirement from `0.5.4` to `0.7.6` in
both manifests. The checked lockfiles currently resolve 0.7.8.

This addresses [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html)
without removing SigV4a or S3 Multi-Region Access Point support. AWS 1.6.3 already
uses p256 0.14, so the signer and its elliptic-curve dependencies can use the
patched crypto-bigint release together. No cryptographic algorithm is rewritten.

## Validation

Run `cargo audit` from the DogRS workspace, and:

```sh
cargo test --manifest-path vendor/aws-sigv4/Cargo.toml --features sigv4a --lib --locked
cargo check -p dog-blob --features s3 --locked
```

The signer suite passes 128 tests, with 10 upstream ignored tests. This includes
AWS SigV4a fixtures and key/signature compatibility checks. It does not constitute
a live AWS Multi-Region Access Point test.

## Upstream

The same dependency fix is already proposed in
[smithy-rs PR #4958](https://github.com/smithy-lang/smithy-rs/pull/4958).
We are tracking that contribution rather than submitting a duplicate.

## Publishing limitation

The root Cargo patch protects builds in this checkout only. Cargo does not carry
it into a published dog-blob dependency. **Do not describe a crates.io release
as fixing this advisory while it still resolves the unpatched AWS signer.**
Applications consuming current crates must apply the same patch at their own
workspace root to receive this temporary fix.

Before releasing the affected S3 integration, obtain an upstream AWS signer
release containing this dependency update, remove the root patch and this
vendor directory, update Cargo.lock, then verify both the workspace and a fresh
consumer without patches using cargo audit. Do not suppress the advisory or
disable SigV4a to make the audit pass.
