#!/usr/bin/env bash
set -euo pipefail

# Elocutio development bootstrap.
# Checks the macOS/Rust/Node toolchain, installs the Rust components we need,
# and installs JavaScript dependencies. It does not install Xcode, Rust, or
# Node itself; it tells you exactly what is missing.

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok() { printf '  \033[32mok\033[0m  %s\n' "$*"; }
warn() { printf '  \033[33mwarn\033[0m %s\n' "$*"; }
fail() { printf '  \033[31mmissing\033[0m %s\n' "$*"; }

missing=0

bold "Elocutio bootstrap"
printf '  macOS: '; sw_vers -productVersion

bold "System"
if command -v xcode-select >/dev/null 2>&1 && xcode-select -p >/dev/null 2>&1; then
  ok "Xcode Command Line Tools at $(xcode-select -p)"
else
  fail "Xcode Command Line Tools (run: xcode-select --install)"
  missing=1
fi
if command -v xcodebuild >/dev/null 2>&1 && xcodebuild -version >/dev/null 2>&1; then
  ok "Xcode $(xcodebuild -version | head -1)"
else
  warn "Full Xcode not detected. Command Line Tools are enough for local builds;"
  warn "install Xcode from the App Store before signing/notarizing releases."
fi

bold "Rust"
if command -v rustup >/dev/null 2>&1; then
  ok "rustup $(rustup --version | head -1)"
  rustup component add rustfmt clippy >/dev/null 2>&1 || warn "could not add rustfmt/clippy"
else
  fail "rustup (install from https://rustup.rs)"
  missing=1
fi
if command -v cargo >/dev/null 2>&1; then
  ok "$(cargo --version)"
else
  fail "cargo (install Rust via rustup)"
  missing=1
fi

bold "Node"
if command -v node >/dev/null 2>&1; then
  ok "Node $(node --version)"
else
  fail "Node 22+ (install from https://nodejs.org or via Homebrew)"
  missing=1
fi
if command -v pnpm >/dev/null 2>&1; then
  ok "pnpm $(pnpm --version)"
else
  fail "pnpm (run: corepack enable)"
  missing=1
fi

bold "Optional tools"
if command -v just >/dev/null 2>&1; then
  ok "just $(just --version | head -1)"
else
  warn "just not installed (brew install just) - optional, but the documented commands use it"
fi
if command -v python3 >/dev/null 2>&1; then
  ok "python3 $(python3 --version)"
else
  warn "python3 not found; docs link checking uses it"
fi

if [[ "$missing" -ne 0 ]]; then
  echo
  bold "Fix the missing tools above, then re-run ./scripts/bootstrap.sh"
  exit 1
fi

bold "Dependencies"
if [[ -f pnpm-lock.yaml ]]; then
  pnpm install --frozen-lockfile
else
  pnpm install
fi
cargo fetch

bold "Verification"
cargo fmt --all -- --check
cargo test --all

echo
bold "Ready"
echo "  Next: just check"
echo "  M0:   ./scripts/fetch-upstream.sh   # shallow-clone Harper v2.13.0 into .upstream/"
