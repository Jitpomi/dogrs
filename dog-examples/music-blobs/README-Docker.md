# Music Blobs with RustFS Docker Setup

This setup provides a complete development environment with RustFS (S3-compatible storage) and the music-blobs service.

## Quick Start

1. **Start the services:**
   ```bash
   docker-compose up -d
   ```

2. **Check service status:**
   ```bash
   docker-compose ps
   ```

3. **View logs:**
   ```bash
   # All services
   docker-compose logs -f
   
   # Specific service
   docker-compose logs -f music-blobs
   docker-compose logs -f rustfs
   ```

## Services

### RustFS (S3-Compatible Storage)
- **API Endpoint:** http://localhost:9000
- **Console:** http://localhost:9001
- **Default Credentials:** 
  - Username: `rustfsadmin`
  - Password: `rustfsadmin`

### Music Blobs Service
- **API Endpoint:** http://localhost:3030
- **Health Check:** http://localhost:3030/health

## API Usage Examples

### Upload Music File
```bash
curl -X POST http://localhost:3030/music/upload \
  -H "Content-Type: application/json" \
  -d '{
    "filename": "song.mp3",
    "content_type": "audio/mpeg",
    "content": "base64_encoded_audio_data_here"
  }'
```

### Download Music File
```bash
curl -X POST http://localhost:3030/music/download \
  -H "Content-Type: application/json" \
  -d '{
    "blob_id": "your_blob_id_here"
  }'
```

## Environment Configuration

The services use these environment variables (configured in docker-compose.yml):

```env
RUSTFS_ENDPOINT_URL=http://rustfs:9000
RUSTFS_REGION=us-east-1
RUSTFS_ACCESS_KEY_ID=rustfsadmin
RUSTFS_SECRET_ACCESS_KEY=rustfsadmin
RUSTFS_BUCKET=music-blobs
```

## Development

### Local Development (without Docker)
1. Copy environment variables:
   ```bash
   cp .env.example .env
   ```

2. Start RustFS only:
   ```bash
   docker-compose up -d rustfs
   ```

3. Run the service locally:
   ```bash
   cargo run
   ```

### Rebuild Services
```bash
# Rebuild and restart
docker-compose up -d --build

# Rebuild specific service
docker-compose build music-blobs
docker-compose up -d music-blobs
```

## Troubleshooting

### Check RustFS Health
```bash
curl http://localhost:9000/minio/health/live
```

### Access RustFS Console
Visit http://localhost:9001 and login with `rustfsadmin` / `rustfsadmin`

### View Service Logs
```bash
docker-compose logs music-blobs
```

### Reset Everything
```bash
docker-compose down -v
docker-compose up -d
```

## Architecture

```
┌─────────────────┐    ┌─────────────────┐
│   Music Blobs   │────│     RustFS      │
│    Service      │    │  (S3-Compatible)│
│   Port: 3030    │    │   Port: 9000    │
└─────────────────┘    └─────────────────┘
```

The music-blobs service uses the RustFS instance as its storage backend through the S3-compatible API.
