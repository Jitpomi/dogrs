# Local Docker setup

From this directory, create a private `.env` with `RUSTFS_ENDPOINT_URL=http://rustfs:9000`,
`RUSTFS_REGION=us-east-1`, `RUSTFS_BUCKET=music-blobs`, and credentials for your local
RustFS service. Never commit the file. Configure the same credentials on RustFS.

```sh
docker compose up -d rustfs
# Create the music-blobs bucket in your local RustFS console.
docker compose up -d --build music-blobs
```

Ports 9000, 9001 and 3030 publish only to 127.0.0.1. The application explicitly
permits binding within its container; this is not authorization for public hosting.
The named `music_state` volume retains receipts, journals and staging across
container restarts. Do not delete it while uploads or recovery records are pending.
The storage image is pinned to the same RustFS digest used by CI.

Upload and download using the current endpoints:

```sh
curl --fail -X POST http://127.0.0.1:3030/uploads \
  -H 'Content-Type: audio/mpeg' -H 'x-filename: song.mp3' --data-binary @song.mp3
# Use the returned receipt id:
curl --fail http://127.0.0.1:3030/blobs/RETURNED_ID -o downloaded.mp3
```

See [README](README.md) for upload limits, native multipart, recovery and the
single-tenant public-demo boundary. No TypeDB server is required for this example.
