#!/bin/bash
# Build binaries OrgInventory Agent macOS (self-contained single-file).
#   ./build-macos.sh [DIST]
# Output: $DIST/{osx-x64,osx-arm64}/OrgInventoryAgent
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DIST="${1:-dist}"
mkdir -p "$DIST"

for RID in osx-x64 osx-arm64; do
    dotnet publish "$HERE/../../macos/src/OrgInventoryAgent.MacOS/OrgInventoryAgent.MacOS.csproj" \
      -c Release -r "$RID" --self-contained true \
      -p:PublishSingleFile=true \
      -p:EnableCompressionInSingleFile=false \
      -o "$DIST/$RID"
done

echo "Built in $DIST/{osx-x64,osx-arm64}/"
