# OCI Container image for HyperMachine API server
# Uses multi-stage build for minimal image size
# Build: podman build -f Containerfile -t ghcr.io/nervosys/hypermachine:latest .
#    or: buildah bud -f Containerfile -t ghcr.io/nervosys/hypermachine:latest .

# --- Build stage ---
FROM rust:1.98-bookworm AS builder

# Install protobuf compiler (required for hv2-api gRPC codegen)
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build only the binaries needed for the container (hv2-cli → hv2, hm-cli → hm)
RUN cargo build --release \
    -p hv2-cli \
    -p hm-cli \
    && strip target/release/hv2 target/release/hm || true

# --- Runtime stage ---
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Non-root user
RUN groupadd -r hypermachine && useradd -r -g hypermachine -d /app hypermachine

WORKDIR /app

# Copy binaries from build stage
COPY --from=builder /build/target/release/hv2 /usr/local/bin/hv2
COPY --from=builder /build/target/release/hm /usr/local/bin/hm

USER hypermachine

EXPOSE 8080 50051 9090

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -f http://localhost:8080/health/live || exit 1

ENTRYPOINT ["hv2"]
