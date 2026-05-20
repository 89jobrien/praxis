default:
    @just --list

# ── Formatting ──────────────────────────────────────────────────────────────

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

# ── Linting ─────────────────────────────────────────────────────────────────

lint:
    cargo clippy --all-targets -- -D warnings

# ── Build ───────────────────────────────────────────────────────────────────

build:
    cargo build --all-targets

# ── Testing ─────────────────────────────────────────────────────────────────

test:
    cargo nextest run

test-crate crate:
    cargo nextest run -p {{crate}}

# ── Gates ───────────────────────────────────────────────────────────────────

# fmt-check + lint + test (CI gate)
ci: fmt-check lint test

# fmt-check + lint + build (pre-commit gate)
pre-commit: fmt-check lint build

# Read-only verify: fmt, clippy, deny
verify: fmt-check lint deny

# ── Dependency audit ────────────────────────────────────────────────────────

deny:
    cargo deny check

# ── Cleanup ─────────────────────────────────────────────────────────────────

clean:
    cargo clean
