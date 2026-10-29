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
	python3 scripts/progress.py validate
	python3 -m unittest discover -s scripts -p "test_*.py"
	pnpm format:check

test:
	cargo test --all

# Type-check the macOS frontend and Tauri backend.
macos-check:
	pnpm --filter @elocutio/macos check
	pnpm --filter @elocutio/macos build
	cargo fmt --manifest-path apps/macos/src-tauri/Cargo.toml -- --check
	cargo clippy --manifest-path apps/macos/src-tauri/Cargo.toml --all-targets -- -D warnings
	cargo check --manifest-path apps/macos/src-tauri/Cargo.toml
	cargo test --manifest-path apps/macos/src-tauri/Cargo.toml

# Build an unsigned debug Elocutio.app.
macos-build:
	pnpm --filter @elocutio/macos build
	cd apps/macos && pnpm exec tauri build --debug --bundles app

# Build a debug Elocutio.app signed with a local identity, so macOS keeps its
# Accessibility grant and Keychain access across rebuilds. List identities with
# `security find-identity -v -p codesigning`.
#   just macos-build-signed "Apple Development: Your Name (TEAMID)"
macos-build-signed identity:
	just macos-build
	just macos-sign "{{identity}}"

# Re-sign the existing debug Elocutio.app with a local identity.
macos-sign identity:
	codesign --force --deep --sign "{{identity}}" apps/macos/src-tauri/target/debug/bundle/macos/Elocutio.app
	codesign --verify --strict apps/macos/src-tauri/target/debug/bundle/macos/Elocutio.app

# Build an optimized app for this Mac's architecture and package it as a DMG,
# the same way the release workflow packages the universal build.
macos-release:
	pnpm --filter @elocutio/macos build
	cd apps/macos && pnpm exec tauri build --bundles app
	./scripts/package-macos.sh apps/macos/src-tauri/target/release/bundle/macos/Elocutio.app apps/macos/src-tauri/target/release/bundle/macos/Elocutio-macOS.dmg

# Run the macOS shell in development mode.
macos-dev:
	cd apps/macos && pnpm tauri dev

# Build the static progress dashboard snapshot (progress/out/).
progress:
	python3 scripts/progress.py build

# Build the publishable dashboard folder (progress/publish/).
progress-publish:
	python3 scripts/progress.py publish

# Republish the dashboard to here.now (requires the here.now skill + API key).
progress-deploy:
	bash ~/.agents/skills/here-now/scripts/publish.sh progress/publish \
		--slug devoted-fable-8gt6 \
		--client deepseek-harness \
		--title "Elocutio — Build Progress" \
		--description "Live build-progress dashboard for Elocutio: milestones, work items, evidence, risks, and git activity."

# Serve the live progress dashboard at http://127.0.0.1:8787.
progress-serve:
	python3 scripts/progress.py serve --open

# Validate the progress ledger and run the tracker tests.
progress-check:
	python3 scripts/progress.py validate
	python3 -m unittest discover -s scripts -p "test_*.py"

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
