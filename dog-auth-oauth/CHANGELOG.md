# Changelog

All notable changes to the `dog-auth-oauth` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.8] — 2026-06-07 — oauth2 5.0 Compatibility

### Changed
- **`oauth2`** bumped from `4.x` → `5.0`
- **`reqwest`** kept at `0.12` to align with oauth2 5.0's internal dependency

### Migration

oauth2 5.0 introduces a type-state pattern — `BasicClient` now carries type parameters
encoding which endpoints have been configured:

```rust
// 0.12 of reqwest is needed to match oauth2 5.0's AsyncHttpClient impl
use oauth2::basic::BasicClient;
use oauth2::{EndpointNotSet, EndpointSet};

// Type alias for a client with auth URL + token URL configured
type ConfiguredClient = BasicClient<
    EndpointSet,      // HasAuthUrl
    EndpointNotSet,   // HasDeviceAuthUrl
    EndpointNotSet,   // HasIntrospectionUrl
    EndpointNotSet,   // HasRevocationUrl
    EndpointSet,      // HasTokenUrl
>;
```

**Before (oauth2 4.x):**
```rust
let client = BasicClient::new(
    ClientId::new(client_id),
    Some(ClientSecret::new(client_secret)),
    AuthUrl::new(auth_url)?,
    Some(TokenUrl::new(token_url)?),
);

// Token exchange
token_req.request_async(async_http_client).await?;
```

**After (oauth2 5.0):**
```rust
let client = BasicClient::new(ClientId::new(client_id))
    .set_client_secret(ClientSecret::new(client_secret))
    .set_auth_uri(AuthUrl::new(auth_url)?)
    .set_token_uri(TokenUrl::new(token_url)?)
    .set_redirect_uri(RedirectUrl::new(redirect_uri)?);

// Token exchange — async_http_client removed, pass reqwest::Client directly
token_req.request_async(&self.http_client).await?;
```

**Key changes:**
- `BasicClient::new` now takes only `ClientId`; other fields are set via builder methods.
- `async_http_client` function removed — pass a `reqwest::Client` directly to `request_async`.
- `authorize_url` and `exchange_code` are only available when the corresponding endpoint type params are `EndpointSet`.
