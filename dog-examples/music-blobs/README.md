# Music blob example

A loopback, single-tenant music application using `dog-transport` and `dog-blob`.
RustFS is an S3-compatible choice made by this application; DogRS does not require it.

## Run

Create a bucket on your existing local S3-compatible service, then provide its
settings through environment variables (never commit credentials):

```sh
export RUSTFS_ENDPOINT_URL=http://127.0.0.1:9000
export RUSTFS_REGION=us-east-1
export RUSTFS_BUCKET=music-blobs
# Also set RUSTFS_ACCESS_KEY_ID and RUSTFS_SECRET_ACCESS_KEY.
cargo run -p music-blobs
```

Open `http://127.0.0.1:3030`. `HTTP_HOST` must be loopback unless the explicit container-only opt-in `MUSIC_ALLOW_CONTAINER_BIND=1` is set; the supplied Compose file publishes only loopback ports. This example deliberately
uses a fixed `default` tenant and does not authenticate users. Add authorization
and derive allowed tenants from verified identity before building a remote service.

## Current upload and download patterns

The browser sends file bodies to `POST /uploads`, with `Content-Type` and an
optional `x-filename`. The server streams those bytes to `BlobAdapter::put`.
DogRS validates and stages the upload once, then uses native provider multipart
when appropriate. It never accepts a filesystem path from request JSON.

`GET /blobs/{id}` streams a download and accepts a single `bytes=start-end` or
`bytes=start-` range. The browser plays that URL directly, rather than copying
whole songs through base64 JSON.

The older `POST /music` custom `upload` method remains available for bounded
multipart/base64 inputs and Dropzone chunks: at most 8 MiB decoded per request,
9 MiB multipart body, and the configured chunk part limit. This path can extract
embedded cover art from a complete small file. Large streamed uploads do not run
that optional extraction. JSON playback/waveform helpers are capped at 8 MiB;
use the streaming route for larger files. Music tags are not automatically indexed
in provider metadata by the generic S3 adapter.

## Resource limits and recovery

Defaults: 100,000,000 bytes per object, four concurrent uploads, 1 GiB shared
staging budget, 120-second upload timeout and 15-second idle timeout. The adapter,
coordinator and S3 store share one `UploadResources` handle. Native parts use a
5 MiB size. Chunk assembly can temporarily retain both parts and the assembled file.

`MUSIC_DATA_DIR` (default `.dogrs-music`) contains private application staging,
receipts and a `FileUploadJournal`. Store it on a trusted writable local filesystem.
Successful uploads persist and fsync the receipt before acknowledging the native
journal record. Startup removes abandoned staging files while preserving live owners.

After a crash, inspect/reconcile native writes using the local administrative command:

```sh
cargo run -p music-blobs -- recover
```

This command skips active writes, recognizes committed objects and aborts known
incomplete multipart uploads. It retains journal records for operator review.
Compare committed records with `receipts/` before acknowledging them in application
code. Unknown provider outcomes must remain pending; do not delete final objects
based on missing business metadata. Provider lifecycle rules can clean up lost
multipart handles. The native journal does not cover every fallback write.

Compatibility chunk sessions and playback controls remain process-local; they are
not a resumable-across-restart or multi-host service. Disk budgets are per shared
process handle, not physical host quotas. See the [blob guide](../../dog-blob/README.md).
