#!/usr/bin/env bash
set -euo pipefail

# Package a built Elocutio.app into a drag-to-install DMG with a SHA-256
# checksum next to it. Used by the release workflow and `just macos-release`.
#
# Usage:
#   ./scripts/package-macos.sh <path/to/Elocutio.app> <path/to/output.dmg>
#
# An app that is not already signed with a real identity (Developer ID, or a
# local one) is ad-hoc signed first: macOS refuses to run unsigned code on
# Apple silicon, and an ad-hoc signature is enough for a user to approve the
# app on first launch.

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <path/to/Elocutio.app> <path/to/output.dmg>" >&2
  exit 2
fi

APP="$1"
DMG="$2"

if [[ ! -d "$APP" ]]; then
  echo "No app bundle at $APP" >&2
  exit 1
fi

if codesign -dv "$APP" 2>&1 | grep -q '^Authority='; then
  echo "Keeping the existing signature:"
  codesign -dv "$APP" 2>&1 | grep '^Authority=' | head -1
else
  echo "Ad-hoc signing $APP"
  codesign --force --deep --sign - "$APP"
fi
codesign --verify --deep --strict "$APP"

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

mkdir -p "$(dirname "$DMG")"
rm -f "$DMG"
hdiutil create -volname Elocutio -srcfolder "$STAGING" -ov -format UDZO "$DMG" >/dev/null

(cd "$(dirname "$DMG")" && shasum -a 256 "$(basename "$DMG")" > "$(basename "$DMG").sha256")
echo "Packaged $DMG"
cat "$DMG.sha256"
