# ── Stage 1: Build (Rust on Debian for compilation) ──
FROM rust:1.93-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config musl-tools \
    && rm -rf /var/lib/apt/lists/*

# Add musl target for static binary
RUN rustup target add x86_64-unknown-linux-musl

# Copy manifests first for dep caching
COPY Cargo.toml Cargo.lock ./
COPY crates/ryvos-core/Cargo.toml crates/ryvos-core/
COPY crates/ryvos-llm/Cargo.toml crates/ryvos-llm/
COPY crates/ryvos-tools/Cargo.toml crates/ryvos-tools/
COPY crates/ryvos-agent/Cargo.toml crates/ryvos-agent/
COPY crates/ryvos-memory/Cargo.toml crates/ryvos-memory/
COPY crates/ryvos-gateway/Cargo.toml crates/ryvos-gateway/
COPY crates/ryvos-tui/Cargo.toml crates/ryvos-tui/
COPY crates/ryvos-mcp/Cargo.toml crates/ryvos-mcp/
COPY crates/ryvos-channels/Cargo.toml crates/ryvos-channels/
COPY crates/ryvos-skills/Cargo.toml crates/ryvos-skills/

# Dummy sources for dep cache
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && \
    for c in ryvos-core ryvos-llm ryvos-tools ryvos-agent ryvos-memory ryvos-gateway ryvos-tui ryvos-mcp ryvos-channels ryvos-skills; do \
      mkdir -p crates/$c/src && echo '' > crates/$c/src/lib.rs; \
    done

RUN cargo build --release --target x86_64-unknown-linux-musl 2>/dev/null || true

# Real source
COPY . .
RUN find src crates -name "*.rs" -exec touch {} +

# Build static binary — stripped, LTO
RUN cargo build --release --target x86_64-unknown-linux-musl --bin ryvos && \
    strip /app/target/x86_64-unknown-linux-musl/release/ryvos

# ── Stage 2: Minimal runtime (Alpine ~5MB base) ──
FROM alpine:3.21

# Only curl + git + ca-certs — no glibc needed (static musl binary)
RUN apk add --no-cache ca-certificates curl git

# Non-root user
RUN adduser -D -h /data ryvos
RUN mkdir -p /data && chown ryvos:ryvos /data

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/ryvos /usr/local/bin/ryvos

USER ryvos
WORKDIR /data

EXPOSE 18789

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:18789/api/health || exit 1

ENTRYPOINT ["ryvos"]
CMD ["daemon", "--gateway"]
