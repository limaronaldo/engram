# Multi-stage Dockerfile for engram-server
# Build: docker build -t engram-server .
# Run:   docker run -v engram-data:/data -p 8080:8080 engram-server

FROM rust:1.83-bookworm AS builder

WORKDIR /build
COPY . .

RUN cargo build --release --bin engram-server --bin engram-cli \
    && strip target/release/engram-server \
    && strip target/release/engram-cli

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/engram-server /usr/local/bin/
COPY --from=builder /build/target/release/engram-cli /usr/local/bin/

ENV ENGRAM_DB_PATH=/data/memories.db
VOLUME /data
EXPOSE 8080

ENTRYPOINT ["engram-server"]
