# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:0.1.77-rust-1.94.0-slim-trixie AS chef
WORKDIR /app
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update -y\
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*


FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY guenther-core/Cargo.toml ./guenther-core/Cargo.toml
COPY telegram/Cargo.toml ./telegram/Cargo.toml
RUN mkdir -p guenther-core/src && touch guenther-core/src/lib.rs\
    && mkdir -p telegram/src && touch telegram/src/main.rs
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder-rs
ARG RUST_FEATURES=""
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo chef cook --release --package telegram ${RUST_FEATURES} --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY guenther-core/Cargo.toml ./guenther-core/Cargo.toml
COPY telegram/Cargo.toml ./telegram/Cargo.toml
COPY guenther-core/src ./guenther-core/src
COPY telegram/src ./telegram/src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --package telegram ${RUST_FEATURES}\
    && strip target/release/telegram \
    && cp target/release/telegram /app/guenther


FROM ghcr.io/astral-sh/uv:0.10.9-debian-slim AS builder-py
ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PYTHON_PREFERENCE=only-managed

RUN uv python install 3.14

RUN --mount=type=cache,target=/root/.cache/uv \
    uv venv --python 3.14 /opt/yt-dlp\
    && uv pip install --python /opt/yt-dlp/bin/python yt-dlp[default] \
    && /opt/yt-dlp/bin/yt-dlp --version


FROM debian:trixie-slim AS runtime

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update -y\
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && useradd -mu 1001 guenther

COPY --from=builder-py /root/.local/share/uv/python /root/.local/share/uv/python
COPY --from=builder-py /opt/yt-dlp /opt/yt-dlp
ENV PATH="/opt/yt-dlp/bin:$PATH"

WORKDIR /app
COPY --from=builder-rs /app/guenther /usr/local/bin/guenther
USER guenther
CMD ["/usr/local/bin/guenther"]
