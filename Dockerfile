# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:latest-rust-slim-trixie AS chef
WORKDIR /app
RUN apt-get update -y\
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*


FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder-rs
ARG RUST_FEATURES
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo chef cook --release ${RUST_FEATURES} --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release ${RUST_FEATURES}\
    && strip target/release/guenther \
    && cp target/release/guenther /app/guenther


FROM ghcr.io/astral-sh/uv:debian-slim AS builder-py
ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PYTHON_INSTALL_DIR=/python \
    UV_PYTHON_PREFERENCE=only-managed

RUN uv python install 3.13

RUN --mount=type=cache,target=/root/.cache/uv \
    uv venv --python 3.14 /opt/yt-dlp\
    && uv pip install --python /opt/yt-dlp/bin/python yt-dlp[default] \
    && /opt/yt-dlp/bin/yt-dlp --version


FROM debian:trixie-slim AS runtime

RUN apt-get update -y\
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*

ENV UV_PYTHON_INSTALL_DIR=/python \
    UV_PYTHON_PREFERENCE=only-managed

COPY --from=builder-py /python /python
COPY --from=builder-py /opt/yt-dlp /opt/yt-dlp
RUN ln -s /opt/yt-dlp/bin/yt-dlp /usr/local/bin/yt-dlp

WORKDIR /app
COPY --from=builder-rs /app/guenther /usr/local/bin/guenther
CMD ["/usr/local/bin/guenther"]
