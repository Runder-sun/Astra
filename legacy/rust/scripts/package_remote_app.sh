#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT_DIR/dist}"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")"
REMOTE_APP_DIR="$ROOT_DIR/src/assets/remote-app"
REMOTE_BUILD_DIR="$ROOT_DIR/src/assets/mobile"
PACKAGE_NAME="astra-remote-app-${VERSION}"
PACKAGE_DIR="$DIST_DIR/$PACKAGE_NAME"
ARCHIVE="$DIST_DIR/${PACKAGE_NAME}.tar.gz"

if [[ "${REMOTE_APP_SKIP_BUILD:-0}" != "1" ]]; then
  npm --prefix "$REMOTE_APP_DIR" ci
  npm --prefix "$REMOTE_APP_DIR" run build
fi

rm -rf "$PACKAGE_DIR" "$ARCHIVE" "$ARCHIVE.sha256"
mkdir -p "$PACKAGE_DIR/icons"

cp "$REMOTE_BUILD_DIR/index.html" "$PACKAGE_DIR/index.html"
cp "$REMOTE_BUILD_DIR/app.js" "$PACKAGE_DIR/app.js"
cp "$REMOTE_BUILD_DIR/styles.css" "$PACKAGE_DIR/styles.css"
cp "$REMOTE_BUILD_DIR/manifest.webmanifest" "$PACKAGE_DIR/manifest.webmanifest"
cp "$REMOTE_BUILD_DIR/sw.js" "$PACKAGE_DIR/sw.js"
cp "$REMOTE_BUILD_DIR/favicon.ico" "$PACKAGE_DIR/favicon.ico"
cp "$REMOTE_BUILD_DIR/icons/icon.svg" "$PACKAGE_DIR/icons/icon.svg"

cat > "$PACKAGE_DIR/package.json" <<JSON
{
  "name": "astra-remote-app",
  "version": "$VERSION",
  "private": true,
  "description": "Installable Astra Remote PWA bundle",
  "scripts": {
    "preview": "python3 -m http.server 8787"
  }
}
JSON

cat > "$PACKAGE_DIR/README.md" <<'EOF'
# Astra Remote App

This package is the static installable remote app shell served by
`research-cli remote daemon`.

Preview locally:

```bash
python3 -m http.server 8787
```

Then open `http://127.0.0.1:8787`.
EOF

tar -C "$DIST_DIR" -czf "$ARCHIVE" "$PACKAGE_NAME"
sha256sum "$ARCHIVE" > "$ARCHIVE.sha256"

printf '%s\n' "$ARCHIVE"
