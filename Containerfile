# syntax=docker/dockerfile:1
#
# OCI Container image for HyperMachine API server
# Uses multi-stage build for minimal image size
#
# hv2-core and hv2-api depend on github.com/nervosys/IronCrypto, which is
# private, so the build needs a token that can read it. It is passed as a
# BuildKit secret rather than a build arg, because a build arg is recorded in
# the image history:
#
#   podman build -f Containerfile \
#       --secret id=ironcrypto_token,env=IRONCRYPTO_TOKEN \
#       -t ghcr.io/nervosys/hypermachine:latest .
#    or: buildah bud -f Containerfile \
#       --secret id=ironcrypto_token,env=IRONCRYPTO_TOKEN \
#       -t ghcr.io/nervosys/hypermachine:latest .

# --- Build stage ---
FROM rust:1.98-bookworm AS builder

# Install protobuf compiler (required for hv2-api gRPC codegen)
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy manifests first for dependency caching. `.cargo/config.toml` has to
# come too: it sets `net.git-fetch-with-cli`, without which cargo's bundled
# libgit2 tries the IronCrypto fetch itself and fails, credentials or not.
COPY Cargo.toml Cargo.lock ./
COPY .cargo/ .cargo/
COPY crates/ crates/

# Build only the binaries needed for the container (hv2-cli → hv2, hm-cli → hm)
#
# The token is mounted for the duration of this one step and never written
# anywhere that survives it. `git config --global` puts it in /root/.gitconfig,
# which would otherwise be committed to the builder layer -- and while the
# builder stage is not shipped, `cache-to: mode=max` does push every stage to
# the registry cache, so that layer is not private. Hence the `rm` in the same
# RUN: a layer records the filesystem as it stands when the step ends.
RUN --mount=type=secret,id=ironcrypto_token \
    set -eu; \
    if [ -s /run/secrets/ironcrypto_token ]; then \
        git config --global \
            url."https://x-access-token:$(cat /run/secrets/ironcrypto_token)@github.com/nervosys/IronCrypto".insteadOf \
            "https://github.com/nervosys/IronCrypto"; \
    fi; \
    cargo build --release -p hv2-cli -p hm-cli; \
    rm -f /root/.gitconfig; \
    strip target/release/hv2 target/release/hm || true

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
