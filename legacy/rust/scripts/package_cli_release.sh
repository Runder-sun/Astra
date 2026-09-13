#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT_DIR/dist}"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")"
TARGET_TRIPLE="${CARGO_BUILD_TARGET:-$(rustc -vV | awk '/^host:/ { print $2; exit }')}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
REMOTE_APP_DIR="$ROOT_DIR/src/assets/remote-app"
REMOTE_BUILD_DIR="$ROOT_DIR/src/assets/mobile"
RELEASE_DIR="$TARGET_DIR/release"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  RELEASE_DIR="$TARGET_DIR/$CARGO_BUILD_TARGET/release"
fi

PACKAGE_NAME="research-cli-${VERSION}-${TARGET_TRIPLE}"
PACKAGE_DIR="$DIST_DIR/$PACKAGE_NAME"
ARCHIVE="$DIST_DIR/${PACKAGE_NAME}.tar.gz"

if [[ "${PACKAGE_SKIP_BUILD:-0}" != "1" ]]; then
  npm --prefix "$REMOTE_APP_DIR" ci
  npm --prefix "$REMOTE_APP_DIR" run build
  cargo build --release --bins
fi

rm -rf "$PACKAGE_DIR" "$ARCHIVE" "$ARCHIVE.sha256"
mkdir -p "$PACKAGE_DIR/bin" "$PACKAGE_DIR/share/astra-remote" "$PACKAGE_DIR/scripts"

cp "$RELEASE_DIR/research-cli" "$PACKAGE_DIR/bin/research-cli"
cp "$RELEASE_DIR/astra" "$PACKAGE_DIR/bin/astra"
cp -R "$REMOTE_BUILD_DIR/." "$PACKAGE_DIR/share/astra-remote/"
cp "$ROOT_DIR/scripts/package_remote_app.sh" "$PACKAGE_DIR/scripts/package_remote_app.sh"

cat > "$PACKAGE_DIR/README.md" <<EOF
# research-cli $VERSION

This package contains:

- \`bin/research-cli\`: compatibility CLI
- \`bin/astra\`: default product CLI
- \`share/astra-remote\`: static installable remote app assets

Quick check:

\`\`\`bash
./bin/research-cli --help
./bin/astra --help
\`\`\`

Remote app:

\`\`\`bash
./bin/research-cli remote daemon --host 127.0.0.1 --port 8787
\`\`\`
EOF

tar -C "$DIST_DIR" -czf "$ARCHIVE" "$PACKAGE_NAME"
sha256sum "$ARCHIVE" > "$ARCHIVE.sha256"

printf '%s\n' "$ARCHIVE"
