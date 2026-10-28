#!/usr/bin/env bash
set -euo pipefail

binary="${1:?binary is required}"
output="${2:?dmg output is required}"
version="${3:?version is required}"

if [[ "$(uname -s)" != Darwin ]]; then
  echo "macOS disk images are built on Darwin" >&2
  exit 1
fi

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/root" "$stage/image"
install -m 755 "$binary" "$stage/root/socli"
pkgbuild \
  --root "$stage/root" \
  --identifier sh.socli.cli \
  --version "$version" \
  --install-location /usr/local/bin \
  --min-os-version 12.0 \
  "$stage/image/socli.pkg" >/dev/null
mkdir -p "$(dirname "$output")"
rm -f "$output"
hdiutil create -quiet -volname "SoCLI" -srcfolder "$stage/image" -ov -format UDZO "$output"
