default:
    @just --list

ci: fmt-check lint test

test:
    cargo nextest run

lint:
    cargo clippy --all-targets -- -D warnings

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

build:
    cargo build --all-targets
