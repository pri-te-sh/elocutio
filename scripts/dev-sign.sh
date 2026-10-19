#!/usr/bin/env bash
set -euo pipefail

# Create or inspect a stable self-signed code-signing certificate for local
# development. A stable identity keeps macOS TCC permissions (Accessibility,
# Screen Recording) from resetting on every rebuild.
#
# Usage:
#   ./scripts/dev-sign.sh          # report status
#   ./scripts/dev-sign.sh --create # create and import "Elocutio Dev"
#
# This modifies your login keychain. It is optional for editing docs, and
# required for comfortable M0/M1 testing.

CERT_NAME="Elocutio Dev"
WORK_DIR="${TMPDIR:-/tmp}/elocutio-dev-sign"

if [[ "${1:-}" != "--create" ]]; then
  if security find-certificate -c "$CERT_NAME" >/dev/null 2>&1; then
    echo "Found '$CERT_NAME' in the keychain."
  else
    echo "No '$CERT_NAME' certificate found."
    echo "Run: ./scripts/dev-sign.sh --create"
  fi
  exit 0
fi

if security find-certificate -c "$CERT_NAME" >/dev/null 2>&1; then
  echo "'$CERT_NAME' already exists; nothing to do."
  exit 0
fi

mkdir -p "$WORK_DIR"
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$WORK_DIR/key.pem" -out "$WORK_DIR/cert.pem" \
  -subj "/CN=$CERT_NAME" \
  -addext "keyUsage=digitalSignature" \
  -addext "extendedKeyUsage=codeSigning"

openssl pkcs12 -export -legacy \
  -inkey "$WORK_DIR/key.pem" -in "$WORK_DIR/cert.pem" \
  -out "$WORK_DIR/identity.p12" -passout pass:elocutio

security import "$WORK_DIR/identity.p12" -k ~/Library/Keychains/login.keychain-db \
  -P elocutio -T /usr/bin/codesign -T /usr/bin/security
security set-key-partition-list -S apple-tool:,apple: -s -k "" \
  ~/Library/Keychains/login.keychain-db >/dev/null 2>&1 || true

echo "Created '$CERT_NAME'. Trust it for code signing in Keychain Access if needed."
echo "Clean up: rm -rf '$WORK_DIR'"
