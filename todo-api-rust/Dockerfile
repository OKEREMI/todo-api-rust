# ---------- Stage 1: builder (has the full Rust toolchain) ----------
FROM rust:1-bookworm AS builder
WORKDIR /app

# Cache trick: build the dependencies first with a dummy main.rs.
# This layer only rebuilds when Cargo.toml or Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Now copy the real code and migrations (migrations are embedded in the binary).
COPY migrations ./migrations
COPY static ./static
COPY src ./src
# Touch main.rs so cargo notices it changed after the dummy build.
RUN touch src/main.rs && cargo build --release

# ---------- Stage 2: runtime (tiny, no compiler) ----------
FROM debian:bookworm-slim

# curl is only here for the HEALTHCHECK.
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin appuser

COPY --from=builder /app/target/release/todo-api-rust /usr/local/bin/todo-api-rust

USER appuser
EXPOSE 3000

HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://localhost:3000/health || exit 1

CMD ["todo-api-rust"]
