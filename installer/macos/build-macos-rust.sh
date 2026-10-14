#!/bin/bash
# Build Rust agent (rust/) cho osx-x64 + osx-arm64 — drop-in thay build-macos.sh (.NET):
# output $DIST/<RID>/OrgInventoryAgent giống `dotnet publish -o dist/<RID>`.
# Chạy trên macOS (cần Xcode Command Line Tools + rustup targets *-apple-darwin).
#   ./build-macos-rust.sh [DIST]
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DIST="$(mkdir -p "${1:-dist}" && cd "${1:-dist}" && pwd)"
WS="$HERE/../../rust"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

for RID in osx-x64 osx-arm64; do
    case "$RID" in
        osx-x64) T=x86_64-apple-darwin ;;
        osx-arm64) T=aarch64-apple-darwin ;;
    esac
    (cd "$WS" && cargo build --release --locked --target "$T")
    mkdir -p "$DIST/$RID"
    install -m 0755 "$WS/target/$T/release/OrgInventoryAgent" "$DIST/$RID/OrgInventoryAgent"
done

echo "Built in $DIST/{osx-x64,osx-arm64}/"
