set shell := ["bash", "-euo", "pipefail", "-c"]

default: check

# Install/verify tooling and dependencies.
bootstrap:
	./scripts/bootstrap.sh

# Format Rust and JS/JSON/YAML.
fmt:
	cargo fmt --all
	pnpm format

# Run every local check used by CI.
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all
	python3 scripts/check_docs.py
	pnpm format:check

test:
	cargo test --all

build:
	cargo build --all

# Regenerate brand and platform icons from assets/brand/source.
assets:
	python3 scripts/build-assets.py

# Shallow-clone the pinned Harper source for the M0 fork spike.
fetch-upstream:
	./scripts/fetch-upstream.sh

# Create a stable self-signed dev certificate for TCC stability.
dev-cert:
	./scripts/dev-sign.sh --create
