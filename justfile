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
alias m := migrate
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
check: fmt-check clippy docs test

# Run the development server
[group("run")]
run:
    bacon run-long

# Run with voice line capture enabled
[group("run")]
run-capture:
    cargo run --no-default-features --features voice-line-capture --release

# sleek reads stdin when invoked without arguments
sqlx_fmt := 'sqlx-inline-fmt --quiet --min-hashes 1 --formatter "sh -c sleek --" src tests'

# Format Rust and inline SQL. The second SQL pass restores indentation after rustfmt.
[group("dev")]
fmt:
    {{ sqlx_fmt }}
    cargo fmt --all
    {{ sqlx_fmt }}

# Check Rust and inline SQL formatting
[group("dev")]
fmt-check:
    cargo fmt --all -- --check
    {{ sqlx_fmt }} --check

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


# Run the full pre-commit workflow
ci: fmt prepare check

# Clean build artifacts
[group("dev")]
clean:
    cargo clean

[group("dev")]
setup:
    mise install

[group("dev")]
bingo-card-preview:
    cargo nextest run --features bingo bingo_card_preview --run-ignored only --no-capture
    @echo "Wrote target/bingo-card-preview.png"

[group("services")]
cobalt-up:
    podman run --rm -p 9000:9000 -e API_URL=http://127.0.0.1:9000/ ghcr.io/imputnet/cobalt:11

[group("container")]
podman-build:
    podman build --tag guenther:local .

# Apply pending migrations to DATABASE_URL
[group("db")]
migrate:
    cargo sqlx migrate run

# Regenerate the offline SQLx query cache (.sqlx)
[group("db")]
prepare: migrate
    cargo sqlx prepare --workspace -- --all-targets --all-features
