# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security and correctness
- Check tenant and actor ownership on every upload-session operation.
- Reject request-supplied filesystem paths; accept trusted handles or bounded byte streams.
- Validate actual sizes before storage using private temporary files.
- Freeze multipart manifests with atomic session revisions; verify part integrity and support completion retries.
- Bound chunk session capacity and lifetime, enforce ordering/count/size rules, and add explicit cleanup.
- Replace date-dependent object keys with stable tenant-separated keys.
- Correct S3 partial-content totals, implement signed reads, and remove whole-object memory buffering.

### Packaging and validation
- Make AWS dependencies optional behind the `s3` feature.
- Add ownership, concurrency, cancellation, corruption, expiry and capacity regression tests.
- Add a live disposable RustFS test and compiled documentation examples.
- Document backend contracts, cleanup, durability limits and migration in README.md.

### Breaking changes
- Existing dated keys require explicit migration or a custom historical key strategy.
- Old multipart sessions must be drained; custom session stores need atomic compare-and-swap.
- New configuration and session fields require updates to struct literals and serialized records.
- Unsupported key hints and application idempotency keys now return errors.

## [0.1.0] - 2024-01-04

### Added
- Initial release of dog-blob
- Core BlobAdapter interface
- S3-compatible storage backend
- Memory storage backend for testing
- Streaming-first architecture
- Multipart/resumable upload support
- Range request support for video streaming
- Multi-tenant context support (BlobCtx)
- Pluggable storage backend system
- Upload coordination and session management
- Comprehensive error handling
- Zero-boilerplate service integration

### Features
- **Streaming uploads/downloads**: Handle large files without memory buffering
- **Multipart coordination**: Automatic multipart uploads for large files
- **Range requests**: First-class support for partial content delivery
- **Storage agnostic**: Works with S3, memory, and custom storage backends
- **Server agnostic**: No HTTP coupling, works with any protocol
- **Production ready**: Used in real-world applications

### Storage Backends
- S3-compatible storage with native multipart support
- Memory storage for testing and development
- Extensible BlobStore trait for custom implementations

### Core Types
- `BlobAdapter`: Main interface for blob operations
- `BlobStore`: Storage backend trait
- `BlobCtx`: Multi-tenant context
- `BlobReceipt`: Portable metadata after storage
- `BlobPut`: Upload request builder
- `ByteRange`: Range request support
