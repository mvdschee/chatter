# The collector's image. Build context: the repo root, so `.dockerignore` there
# decides what goes in (only the collector's crate and migrations). Built and pushed by
# .github/workflows/collector-image.yml.

# Same Rust as rust-toolchain.toml. That file isn't copied in: it also lists the
# firmware targets, which the collector doesn't need.
FROM rust:1.99-bookworm AS builder
WORKDIR /app
COPY crates/collector/ ./
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime
# nmap for the network scans. The container runs as root, on the host's
# network, with every capability dropped except NET_RAW and NET_ADMIN (set by
# the deployment, see DECISIONS.md): nmap's ARP sweep needs raw sockets.
RUN apt-get update && \
    apt-get install -y --no-install-recommends nmap && \
    rm -rf /var/lib/apt/lists/* && \
    mkdir -p /var/lib/chatter

COPY --from=builder /app/target/release/chatter-collector /usr/local/bin/chatter-collector

EXPOSE 8080
ENV CHATTER_BIND=0.0.0.0:8080 \
    CHATTER_DB=/var/lib/chatter/chatter.db
CMD ["chatter-collector"]
