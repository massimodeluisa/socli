#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?release version is required}"
: "${TARGET:?Rust target is required}"

archive="socli-${VERSION}-${TARGET}"
mkdir -p "release/${archive}" artifacts
install -m 755 "target/${TARGET}/release/socli" "release/${archive}/socli"
cp LICENSE README.md THIRD-PARTY-NOTICES.md CHANGELOG.md "release/${archive}/"
tar -czf "artifacts/${archive}.tar.gz" -C release "$archive"

if [[ "$TARGET" == *linux* ]]; then
  : "${PACKAGE_ARCH:?package architecture is required}"
  : "${PACKAGE_MAINTAINER:?package maintainer is required}"
  mkdir -p release/package
  install -m 755 "target/${TARGET}/release/socli" release/package/socli
  for format in deb rpm apk; do
    nfpm package --config packaging/nfpm.yaml --packager "$format" --target artifacts/
  done
fi
