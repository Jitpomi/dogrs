# dog-blob

[![Crates.io](https://img.shields.io/crates/v/dog-blob.svg)](https://crates.io/crates/dog-blob)
[![Documentation](https://docs.rs/dog-blob/badge.svg)](https://docs.rs/dog-blob)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

**Object storage adapter for DogRS - blob storage, multipart uploads, and file handling**

dog-blob provides object storage capabilities for the DogRS ecosystem with multipart uploads, range requests, and pluggable storage backends.

## Features

- **Multipart uploads** - Handle large files with resumable uploads
- **Range requests** - Support for streaming and partial content
- **Storage backends** - Pluggable storage (S3, filesystem, custom)
- **Production-ready** - Built for high-throughput applications
- **DogRS integration** - Works seamlessly with DogRS services

## Quick Start

```bash
cargo add dog-blob
```

## Examples

See `dog-examples/music-blobs` for a complete implementation.

## Architecture

```
┌─────────────────┐
│   Your App      │  ← Business logic
└─────────────────┘
         │
         ▼
┌─────────────────┐
│   dog-blob      │  ← Object storage adapter
│   (Storage)     │
└─────────────────┘
         │
         ▼
┌─────────────────┐
│   dog-core      │  ← Core abstractions
└─────────────────┘
```

## License

MIT OR Apache-2.0

---

<div align="center">

**Made by [Jitpomi](https://github.com/Jitpomi)**

</div>
