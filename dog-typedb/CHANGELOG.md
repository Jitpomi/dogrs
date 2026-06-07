# Changelog

All notable changes to the `dog-typedb` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.8] — 2026-06-07 — TypeDB Driver 3.11 Compatibility

### Changed
- **`typedb-driver`** bumped from `3.7.0` → `3.11.5`

### Migration

`TypeDBDriverFactory::connect` (and any direct driver construction) requires two API changes:

**Before:**
```rust
let options = DriverOptions::new(tls, None).map_err(|e| anyhow!(e))?;
TypeDBDriver::new(address, credentials, options).await?
```

**After:**
```rust
use typedb_driver::{Addresses, DriverOptions, DriverTlsConfig};

let tls_config = if tls { DriverTlsConfig::default() } else { DriverTlsConfig::disabled() };
let options = DriverOptions::new(tls_config);
let addresses = Addresses::try_from_address_str(address).map_err(|e| anyhow!(e))?;
TypeDBDriver::new(addresses, credentials, options).await?
```

**Key changes:**
- `DriverOptions::new` no longer accepts a `bool` — use `DriverTlsConfig::default()` (TLS on, system certs) or `DriverTlsConfig::disabled()`.
- `TypeDBDriver::new` now requires an `Addresses` object; pass a raw `&str` address via `Addresses::try_from_address_str`.
