#!/bin/bash
# Build this machine's release artifacts locally: app bundle, DMG, CLI archive.
#
#   scripts/build-local.sh                 # -> dist/local/
#
# No GitHub runner and no Finder automation: the DMG is built with hdiutil
# from a staging folder, so it works over SSH and in CI shells alike.
# Windows, Linux and Intel macOS bundles cannot be produced on an Apple
# Silicon Mac; use the CI workflow for those.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

V=$(python3 - <<'PY'
import json
print(json.load(open("app/src-tauri/tauri.conf.json"))["version"])
PY
)
[ -n "$V" ] || { echo "could not read the version" >&2; exit 1; }

ARCH=$(uname -m)
case "$ARCH" in
  arm64) NAME=arm64; TARGET=aarch64-apple-darwin ;;
  x86_64) NAME=x64; TARGET=x86_64-apple-darwin ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac

OUT="dist/local"
rm -rf "$OUT"
mkdir -p "$OUT"

echo "== tests"
cargo test --workspace

echo "== frontend"
npm --prefix app install --no-audit --no-fund
npm --prefix app run build

echo "== CLI"
cargo build --release -p godsend-ps5-cli
tar -czf "$OUT/godsend-ps5-cli-macos-$NAME.tar.gz" -C target/release godsend-ps5

echo "== app bundle"
npm --prefix app run tauri build -- --bundles app
APP="target/release/bundle/macos/GODsend PS5.app"
[ -d "$APP" ] || { echo "no app bundle at $APP" >&2; exit 1; }
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

echo "== DMG (hdiutil, no Finder)"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$OUT/godsend-ps5-macos-$NAME.dmg"
hdiutil create -volname "GODsend PS5" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null

echo "== done"
find "$OUT" -type f -maxdepth 1 -exec ls -lh {} \; | awk '{print $5, $9}'
echo
echo "publish all artifacts (local plus any CI ones in dist/) with:"
echo "  scripts/publish-release.sh --artifacts dist"
