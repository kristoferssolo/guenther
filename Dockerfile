FROM lukemathwalker/cargo-chef:latest-rust-slim-trixie AS chef
WORKDIR /app
RUN apt-get update -y\
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*


FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder-rs
ARG RUST_FEATURES
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release ${RUST_FEATURES} --recipe-path recipe.json
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
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
    uv tool install yt-dlp[default]\
    && yt-dlp --version


FROM debian:trixie-slim AS runtime

RUN apt-get update -y\
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg curl unzip npm \
    && rm -rf /var/lib/apt/lists/*

ENV UV_PYTHON_INSTALL_DIR=/python \
    UV_PYTHON_PREFERENCE=only-managed \
    PATH="/root/.local/bin:${PATH}"

COPY --from=builder-py /python /python
COPY --from=builder-py /root/.local /root/.local

WORKDIR /app
COPY --from=builder-rs /app/guenther /usr/local/bin/guenther
CMD ["/usr/local/bin/guenther"]
