#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
    echo "run-macos.sh must run on macOS" >&2
    exit 1
fi

binary=${1:-./omnibuild}
if [ "$#" -gt 0 ]; then
    shift
fi

if [ ! -f "$binary" ]; then
    echo "artifact not found: $binary" >&2
    exit 1
fi

xattr -dr com.apple.quarantine "$binary" 2>/dev/null || true

if command -v codesign >/dev/null 2>&1; then
    if ! codesign --verify "$binary" >/dev/null 2>&1; then
        echo "ad-hoc signature missing; signing $binary" >&2
        codesign --force --sign - "$binary"
    fi
fi

exec "$binary" "$@"
