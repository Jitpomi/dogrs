# AWS SigV4a security fix and release status

Last verified: 25 October 2026. Implementation: DogRS commit
`f3cdfa59ff9463e948a0cc77c31d518f8b983ef4`, in
[PR #27](https://github.com/Jitpomi/dogrs/pull/27).

## What is fixed

AWS SigV4a is the signing mechanism used by S3 Multi-Region Access Points.
Its dependency tree included crypto-bigint versions affected by
[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html),
a timing-side-channel advisory. This is a dependency security fix, not a finding
that Multi-Region Access Points themselves are broken or that a compromise occurred.

DogRS temporarily vendors aws-sigv4 1.6.3 and raises its crypto-bigint requirement
to at least 0.7.6. Both checked lockfiles resolve 0.7.8. Signing algorithms and
AWS fixtures are unchanged; SigV4a stays enabled. The same dependency change is
proposed in [upstream PR #4958](https://github.com/smithy-lang/smithy-rs/pull/4958).
At the verification date that PR remained open and the latest published signer
was 1.6.3. Our copy is a local backport, not an official AWS release.

## Who receives the fix

| Build | Status |
| --- | --- |
| This DogRS checkout, with its root patch and lockfile | Patched dependency is selected |
| Existing crates.io consumers | Do not inherit the repository patch; inspect their own dependency graph |
| Application with the same patch at its workspace root | Can select the patched signer after updating and verifying its lockfile |
| dog-blob without the optional `s3` feature | Does not introduce this AWS signer; other application dependencies may still do so |

A clean audit here does **not** establish that published DogRS packages are fixed.
Cargo only applies `[patch]` entries from the consuming workspace root. It does
not propagate a library repository's override to downstream applications.

## Temporary application workaround

Use a reviewed copy of `vendor/aws-sigv4` from the implementation commit above.
Copy the entire directory into your application's `vendor/aws-sigv4`, retaining
its license, source, fixtures, tests and patch notes. Add this to your application's
root Cargo.toml (merge with an existing patch table if present):

```toml
[patch.crates-io]
aws-sigv4 = { path = "vendor/aws-sigv4" }
```

Then run from that application workspace:

```sh
cargo update -p aws-sigv4
cargo tree -i aws-sigv4
cargo audit
cargo test --locked
```

Check that the tree selects your local signer, review and commit the lockfile,
and run your normal S3 integration checks. This workaround means maintaining a
local dependency patch until AWS releases the fix. Do not ignore the advisory
or assume that merely updating dog-blob installs the patched signer.

## Verification and limits

Local validation at the implementation commit:

- Zero vulnerability findings in the workspace cargo audit. This is not a claim
  that every audit warning is resolved or that the application has no security bugs.
- 131 AWS signer library tests, one dependency-closure test, two MRAP signing
  tests and two doc tests passed. Ten upstream tests remain ignored.
- The dependency test checks transitive as well as direct crypto-bigint versions.
- The MRAP tests verify header and query signatures against AWS's published public
  key, preserve region sets and regionless credential scopes, and reject tampering.
- The offline S3 SDK regression covers MRAP and regional presigning with a
  synthetic session token. No network request or real AWS credentials are used.
- Strict signer Clippy, S3 compilation, and 37 dog-blob tests/doc tests passed.
  The live RustFS test was ignored locally; no live AWS MRAP test was performed.

These checks validate dependency removal and signing compatibility. They do not
prove constant-time behavior on every CPU/compiler, certify a production AWS
configuration, or add a new MRAP configuration API to dog-blob's S3-compatible store.
GitHub CI has its own results in PR #27; local test results are not a CI pass claim.

## Removing the temporary patch

Before releasing an S3 security fix for crates.io consumers:

1. Verify that AWS published a signer release containing the corrected dependency.
   Do not assume a proposed version number means the release exists.
2. Update DogRS to select that release. Ensure the published dependency constraints
   require a fixed signer; a workspace lockfile alone does not constrain consumers.
3. Remove the root override, vendor directory and vendor-only CI step. Retain the
   SDK signing regression and update these notes with the replacement release.
4. Run the workspace audit, S3 tests and relevant CI checks.
5. Package dog-blob and test a fresh application with the `s3` feature, using only
   registry dependencies and no root patches. Inspect its signer dependency tree
   and run cargo audit. Also test updating a consumer with an existing lockfile.
6. Publish only after those checks pass, and name the fixed dependency versions
   in the release notes. Until then, do not describe crates.io consumers as covered.

See [patch provenance](../vendor/aws-sigv4/DOGRS-PATCH.md) for the exact imported
upstream test commit and license details.
