# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Multi-stage Docker Build
#
# Targets: linux/amd64, linux/arm64 (Oracle Cloud ARM Ampere)
# Base: Debian slim for minimal attack surface
# ──────────────────────────────────────────────────────────────

# ── Stage 1: Build ───────────────────────────────────────────
FROM rust:1.82-bookworm AS builder

WORKDIR /build

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY protocol/Cargo.toml protocol/Cargo.toml
COPY client/Cargo.toml client/Cargo.toml
COPY proxy/Cargo.toml proxy/Cargo.toml

# Create dummy source files to cache dependency compilation
RUN mkdir -p protocol/src client/src proxy/src && \
    echo "pub fn lib() {}" > protocol/src/lib.rs && \
    echo "fn main() {}" > client/src/main.rs && \
    echo "fn main() {}" > proxy/src/main.rs && \
    echo "" > proxy/src/lib.rs

# Build dependencies only (cached layer)
RUN cargo build --release --bin lightspeed-proxy 2>/dev/null || true

# Now copy real source code
COPY protocol/ protocol/
COPY proxy/ proxy/

# Touch source files to invalidate the cache for our code only
RUN touch protocol/src/lib.rs proxy/src/main.rs proxy/src/lib.rs

# Build the proxy binary
RUN cargo build --release --bin lightspeed-proxy && \
    strip /build/target/release/lightspeed-proxy

# ── Stage 2: Runtime ─────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Install minimal runtime dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd --gid 1000 lightspeed && \
    useradd --uid 1000 --gid lightspeed --shell /bin/false lightspeed

# Copy binary
COPY --from=builder /build/target/release/lightspeed-proxy /usr/local/bin/lightspeed-proxy

# Create config directory
RUN mkdir -p /etc/lightspeed && chown lightspeed:lightspeed /etc/lightspeed

# Default configuration
COPY proxy/proxy.toml.default /etc/lightspeed/proxy.toml

# Switch to non-root user
USER lightspeed

# Expose ports
#   4434/udp  — Data plane (tunnel traffic)
#   4433/udp  — QUIC control plane
#   8080/tcp  — Health check HTTP
EXPOSE 4434/udp
EXPOSE 4433/udp
EXPOSE 8080/tcp

# Health check
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8080/health || exit 1

ENTRYPOINT ["lightspeed-proxy"]
CMD ["--config", "/etc/lightspeed/proxy.toml"]
