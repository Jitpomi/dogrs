# Use official Rust image as builder
FROM rust:1.90-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /workspace

# Copy workspace files (build context should be from dogrs root)
COPY . .

# Build the music-blobs binary
WORKDIR /workspace/dog-examples/music-blobs
RUN cargo build --release

# Runtime stage
FROM debian:trixie-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false -m -d /app appuser

# Set working directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /workspace/target/release/music-blobs /app/music-blobs

# Copy static files if they exist
COPY --from=builder /workspace/dog-examples/music-blobs/static /app/static

# Change ownership to app user
RUN chown -R appuser:appuser /app

# Switch to app user
USER appuser

# Expose port
EXPOSE 3030

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3030/health || exit 1

# Run the application
CMD ["./music-blobs"]
