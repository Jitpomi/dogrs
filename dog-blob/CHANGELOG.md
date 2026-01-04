# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive documentation with real-world examples
- Performance and best practices guide
- API reference documentation
- Video streaming service example
- Document management system example
- Backup service example with chunked uploads
- Error handling patterns and retry logic
- Monitoring and observability examples
- Configuration tuning guidelines

### Enhanced
- README.md with table of contents and better structure
- Storage backends documentation with feature comparisons
- Multipart upload examples (automatic and manual)
- Range request examples for video/audio scrubbing
- Metadata and custom fields documentation
- lib.rs documentation with comprehensive examples

### Improved
- Cargo.toml metadata for better discoverability
- Code examples with proper error handling
- Architecture diagrams and explanations
- Performance optimization recommendations

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
