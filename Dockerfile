# ============================================================
# Stage 1: Build the Rust binaries
# ============================================================
FROM rust:slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better Docker layer caching
COPY Cargo.toml Cargo.lock* ./

# Create a dummy main.rs to pre-build dependencies
RUN mkdir -p src/bin && \
    echo "fn main() {}" > src/main.rs && \
    echo "fn main() {}" > src/bin/build_index.rs && \
    RUSTFLAGS="-C target-cpu=x86-64-v3" cargo build --release 2>/dev/null || true && \
    rm -rf src

# Copy actual source code
COPY src/ src/
RUN touch src/main.rs src/bin/build_index.rs src/models.rs src/index.rs src/search.rs src/decision.rs src/vectorize.rs

# Build both binaries with optimizations
RUN RUSTFLAGS="-C target-cpu=x86-64-v3" cargo build --release

# ============================================================
# Stage 2: Build the index from references.json.gz
# ============================================================
FROM debian:bookworm-slim AS indexer

COPY --from=builder /app/target/release/build_index /usr/local/bin/build_index
COPY resources/references.json.gz /data/references.json.gz

RUN build_index /data/references.json.gz /data/index.bin

# ============================================================
# Stage 3: Minimal runtime image
# ============================================================
FROM debian:bookworm-slim

# Copy the API binary
COPY --from=builder /app/target/release/api /usr/local/bin/api

# Copy the pre-built index and config files
COPY --from=indexer /data/index.bin /data/index.bin
COPY resources/mcc_risk.json /data/mcc_risk.json
COPY resources/normalization.json /data/normalization.json

# Install wget for healthcheck
RUN apt-get update && apt-get install -y --no-install-recommends wget && \
    rm -rf /var/lib/apt/lists/*

ENV PORT=8080
ENV INDEX_PATH=/data/index.bin
ENV NORM_PATH=/data/normalization.json
ENV MCC_PATH=/data/mcc_risk.json

EXPOSE 8080

CMD ["api"]
