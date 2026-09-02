# syntax=docker/dockerfile:1.7

FROM rust:1.94.0-slim-trixie AS builder-rs
ARG RUST_FEATURES=""
WORKDIR /app
# Offline sqlx keeps the builder free of a database and of `sqlx-cli`; the query
# cache in `.sqlx` is regenerated with `cargo sqlx prepare`.
ENV CARGO_INCREMENTAL=0 \
    SQLX_OFFLINE=true

COPY Cargo.toml Cargo.lock ./
COPY .sqlx ./.sqlx
COPY migrations ./migrations
COPY src ./src

# The target directory and the cargo registry live in build cache mounts, so a
# rebuild only recompiles this crate. Nothing here survives into the image, which
# is why the binary is copied out of the mount before the layer is committed.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --locked --release --bin guenther ${RUST_FEATURES} \
    && cp target/release/guenther /app/guenther


FROM debian:trixie-slim AS runtime

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update -y\
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && useradd -mu 1001 guenther \
    && install -d -o guenther -g guenther /app/data

WORKDIR /app
COPY --from=builder-rs /app/guenther /usr/local/bin/guenther
USER guenther
CMD ["/usr/local/bin/guenther"]
