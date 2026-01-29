# HyperMachine MCP Server Docker Image
#
# Build:
#   docker build -t hypermachine:latest .
#
# Run:
#   docker run -p 8080:8080 -e HM_API_KEY=your-secret-key hypermachine:latest
#
# With persistent state:
#   docker run -p 8080:8080 -v hypermachine-data:/data -e HM_API_KEY=your-key hypermachine:latest

# Build stage
FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build release binary
RUN cargo build --release --package hm-cli

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 hypermachine

# Copy binary from builder
COPY --from=builder /app/target/release/hm /usr/local/bin/hm

# Create data directory
RUN mkdir -p /data && chown hypermachine:hypermachine /data

# Switch to non-root user
USER hypermachine

# Set environment variables
ENV LOCALAPPDATA=/data
ENV RUST_LOG=info

# Expose MCP server port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Default command - start MCP server
ENTRYPOINT ["hm"]
CMD ["serve", "--rest-port", "8080"]
