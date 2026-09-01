export RUSTC_WRAPPER :=  env("RUSTC_WRAPPER", "sccache")
export RUST_LOG := env("RUST_LOG", "warn")

set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes
default:
    @just --list

alias b := build
alias c := check
alias d := docs
alias f := fmt
alias r := run
alias t := test

[group("build")]
build:
    cargo build

[group("build")]
build-release:
    cargo build --release

[group("build")]
build-all:
    cargo build --release --all-features

# Run all checks with every feature enabled
[group("dev")]
check: clippy docs test

# Run the development server
[group("run")]
run:
    bacon run-long

# Format code
[group("dev")]
fmt:
    cargo fmt --all

# Run clippy
[group("dev")]
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Build documentation
[group("dev")]
docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

[group("dev")]
docs-default:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Run tests with nextest
[group("dev")]
test:
    cargo nextest run --all-features

[group("dev")]
test-default:
    cargo nextest run

# Clean build artifacts
[group("dev")]
clean:
    cargo clean

[group("dev")]
setup:
    cargo install cargo-nextest sccache

[group("dev")]
bingo-card-preview:
    cargo test --features bingo bingo_card_preview -- --ignored --nocapture
    @echo "Wrote target/bingo-card-preview.png"

[group("services")]
cobalt-up:
    podman run --rm -p 9000:9000 -e API_URL=http://127.0.0.1:9000/ ghcr.io/imputnet/cobalt:11

[group("container")]
docker-build:
    docker build --tag guenther:local .
