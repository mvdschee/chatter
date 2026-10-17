# The collector's image. Build context: the repo root, so `.dockerignore` there
# decides what goes in (only crates/collector). Built and pushed by
# .github/workflows/collector-image.yml.

# Same Rust as rust-toolchain.toml. That file isn't copied in: it also lists the
# firmware targets, which the collector doesn't need.
FROM rust:1.99-bookworm AS builder
WORKDIR /app
COPY crates/collector/ ./
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime
# a nonroot user that owns only the data dir
RUN groupadd --gid 65532 chatter && \
    useradd --uid 65532 --gid 65532 --no-create-home --shell /usr/sbin/nologin chatter && \
    mkdir -p /var/lib/chatter && \
    chown 65532:65532 /var/lib/chatter

COPY --from=builder /app/target/release/chatter-collector /usr/local/bin/chatter-collector

EXPOSE 8080
USER chatter
CMD ["chatter-collector", "0.0.0.0:8080", "--out", "/var/lib/chatter/reports.jsonl"]
