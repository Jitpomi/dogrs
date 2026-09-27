# dog-axum — deprecated compatibility wrapper

DogRS has moved shared HTTP handling to **`dog-transport`**, a web-server-agnostic
crate built on HTTP/Tower interfaces. Use it directly with Axum or another
compatible host for new applications. Axum is optional and is not a production
dependency of `dog-transport`.

This crate remains for existing users while they migrate. REST and custom/OAuth
calls now delegate to `dog-transport`; routing convenience APIs remain Axum-specific.
It is deprecated in documentation and package metadata, without compiler warnings
that would break consumers using `-D warnings`. No removal date is set.

See the [migration guide](../dog-transport/MIGRATION.md) for dependencies, complete
Axum examples, aliases, parameters/auth, middleware, custom methods/OAuth, and uploads.
On crates.io/docs.rs, use the [repository migration guide](https://github.com/Jitpomi/dogrs/blob/main/dog-transport/MIGRATION.md).

For existing source consumers, `AxumApp`, `axum`, `RestParams`, `FromRestParams`, and
custom-route helpers remain available. The `auth` feature retains the legacy
`AuthParams<RestParams>` conversion trait. New code needs only Serde-compatible
parameters and `dog-transport`'s `http` feature.

The legacy multipart helper is retained for transition and now enforces limits,
preserves requests, runs processors, and cleans temporary files. Its defaults and
file lifetime have changed: read the upload migration section before upgrading.
Neither this wrapper nor the transport grants tenant access based on headers;
install your application's authentication and authorization policies.
