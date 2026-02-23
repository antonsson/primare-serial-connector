# Multi-stage Dockerfile for primare-i22-rs232
# Uses Alpine for minimal image size (~15MB)
# Supports cross-compilation to ARM64 (Raspberry Pi 5) via docker buildx

# Build stage - Alpine with Rust
FROM rust:1.75-alpine AS builder

WORKDIR /app

# Install build dependencies (musl-dev for static linking)
RUN apk add --no-cache musl-dev pkgconfig eudev-dev

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock* ./

# Create dummy src to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src target/release/deps/primare*

# Copy actual source and build
COPY src ./src
RUN cargo build --release

# Runtime stage - minimal Alpine
FROM alpine:3.19

RUN apk add --no-cache ca-certificates curl eudev-libs

COPY --from=builder /app/target/release/primare-i22-rs232 /usr/local/bin/

ENV PRIMARE_PORT=/dev/ttyUSB0
ENV PRIMARE_BAUD=4800
ENV PRIMARE_LISTEN=0.0.0.0:3000
ENV PRIMARE_TIMEOUT_MS=500
ENV RUST_LOG=primare_i22_rs232=info

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

ENTRYPOINT ["primare-i22-rs232"]
