#!/usr/bin/env bash
set -euo pipefail

# Fetch the pinned Harper source for the M0 fork spike.
# The checkout is gitignored; it is a reference/source tree, not a vendored
# directory we commit.

HARPER_TAG="${HARPER_TAG:-v2.13.0}"
DEST="${1:-.upstream/harper}"

if ! command -v git >/dev/null 2>&1; then
  echo "git is required" >&2
  exit 1
fi

if [[ -d "$DEST/.git" ]]; then
  echo "Harper already present at $DEST"
  git -C "$DEST" describe --tags --always 2>/dev/null || true
  exit 0
fi

mkdir -p "$(dirname "$DEST")"
echo "Cloning Harper $HARPER_TAG into $DEST ..."
git clone --depth 1 --branch "$HARPER_TAG" \
  https://github.com/Automattic/harper.git "$DEST"

echo
echo "Harper $HARPER_TAG is ready at $DEST"
echo "Next: inspect $DEST/harper-desktop and $DEST/harper-desktop/src-tauri/src/mac_broker."
