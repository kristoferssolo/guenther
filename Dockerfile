FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app


FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder-rs
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --no-default-features --features tiktok --features twitter --features youtube --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --no-default-features --features tiktok --features twitter --features youtube


FROM ghcr.io/astral-sh/uv:trixie-slim AS builder-py
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy

# Configure the Python directory so it is consistent
ENV UV_PYTHON_INSTALL_DIR=/python

# Only use the managed Python version
ENV UV_PYTHON_PREFERENCE=only-managed

# Install Python before the project for caching
RUN uv python install 3.13

RUN apt-get update -y\
    && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates ffmpeg curl unzip\
    && rm -rf /var/lib/apt/lists/*

# Install Deno (required by yt-dlp for YouTube challenge solving)
RUN curl -fsSL https://deno.land/install.sh | DENO_INSTALL=/usr/local sh

RUN --mount=type=cache,target=/root/.cache/uv

# Intstall deps
RUN uv tool install yt-dlp[default]\
    && yt-dlp --version

WORKDIR /app
COPY --from=builder-rs /app/target/release/guenther /usr/local/bin/guenther
CMD ["/usr/local/bin/guenther"]

