# HyperMachine Container Image
# Multi-stage build for minimal production image

# Build stage
FROM rust:1.75-slim-bookworm AS builder

ARG VERSION=0.0.0-dev

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release binary
RUN cargo build --release --package hv2-api

# Runtime stage
FROM debian:bookworm-slim AS runtime

ARG VERSION=0.0.0-dev

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r hypermachine \
    && useradd -r -g hypermachine -d /app hypermachine

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/hv2-api /app/hypermachine

# Create necessary directories
RUN mkdir -p /app/data /app/config \
    && chown -R hypermachine:hypermachine /app

# Security: Run as non-root
USER hypermachine

# Environment
ENV RUST_LOG=info
ENV ENVIRONMENT=production
ENV VERSION=${VERSION}

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD ["/app/hypermachine", "health-check"]

# Expose ports
EXPOSE 8080 9090

# Labels
LABEL org.opencontainers.image.title="HyperMachine" \
      org.opencontainers.image.description="High-performance hypervisor" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.vendor="Nervosys" \
      org.opencontainers.image.source="https://github.com/nervosys/hypermachine"

# Entry point
ENTRYPOINT ["/app/hypermachine"]
CMD ["serve", "--bind", "0.0.0.0:8080"]
