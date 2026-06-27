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
    cargo build --release

# Run all checks (fmt, clippy, docs, test)
[group("dev")]
check: fmt clippy docs test

# Run the development server
[group("run")]
run:
    cargo run

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

# Run tests with nextest
[group("dev")]
test:
    cargo nextest run --all-features

# Clean build artifacts
[group("dev")]
clean:
    cargo clean

[group("dev")]
setup:
    cargo install cargo-nextest sccache
