# AWS SigV4a security and upstream signing regressions

Verified 16 November 2026. The workspace now resolves the published AWS
`aws-sigv4` 1.6.5 release and patched `crypto-bigint`. The temporary 1.6.3
vendor fork and root Cargo patch are retired. No signing algorithm, SigV4a
feature or S3 Multi-Region Access Point capability was removed.

The prior backport, its provenance and fixtures remain in Git history. Its
independent dependency-closure and MRAP header/query signature tests now live in
`tests/aws-signing`, against the registry signer. They preserve the imported
Amazon Apache-2.0 notices and verify AWS's public key and rejection of tampering.
CI runs these tests and audits the standalone fixture lockfile as well as the
workspace. This is offline signing verification, not a live AWS MRAP test.

```sh
cargo test --manifest-path tests/aws-signing/Cargo.toml --all-features --locked
cargo audit --file tests/aws-signing/Cargo.lock --deny unsound --deny yanked
cargo audit --deny unsound --deny yanked
```

Consumers must update and audit their own lockfiles. Repository Cargo patches
never protected published consumers; the current tests use registry dependencies
without a patch or vendored implementation. `dog-blob` without `s3` continues to
exclude AWS dependencies. S3 upload/download/range tests remain part of CI.
