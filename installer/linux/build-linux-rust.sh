#!/bin/bash
# Build Rust agent (rust/) cho 4 RID Linux — drop-in thay build-linux.sh (.NET):
# output dist/<RID>/OrgInventoryAgent giống `dotnet publish -o dist/<RID>`.
# Cần: rustup targets + cargo-zigbuild + zig (pip install ziglang).
# glibc build pin 2.17 để chạy được trên distro cũ (RHEL/CentOS 7+).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DIST="$(mkdir -p "${1:-dist}" && cd "${1:-dist}" && pwd)"
WS="$HERE/../../rust"

declare -A TARGETS=(
    [linux-x64]="x86_64-unknown-linux-gnu.2.17"
    [linux-arm64]="aarch64-unknown-linux-gnu.2.17"
    [linux-musl-x64]="x86_64-unknown-linux-musl"
    [linux-musl-arm64]="aarch64-unknown-linux-musl"
)

for RID in linux-x64 linux-arm64 linux-musl-x64 linux-musl-arm64; do
    T="${TARGETS[$RID]}"
    (cd "$WS" && cargo zigbuild --release --locked --target "$T")
    mkdir -p "$DIST/$RID"
    install -m 0755 "$WS/target/${T%.2.17}/release/OrgInventoryAgent" "$DIST/$RID/OrgInventoryAgent"
done

echo "Built in $DIST/{linux-x64,linux-arm64,linux-musl-x64,linux-musl-arm64}/"
