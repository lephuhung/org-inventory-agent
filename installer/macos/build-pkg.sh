#!/bin/bash
# Build gói .pkg (unsigned) cho OrgInventory Agent macOS.
#   ./build-pkg.sh [RID] [OUT] [VERSION]
#     RID: osx-arm64 (mặc định) | osx-x64
#     VERSION: bắt buộc nếu không truyền → đọc từ csproj <Version>.
#
# Chạy trên macOS (cần pkgbuild). Gói unsigned giống MSI — Gatekeeper sẽ
# cảnh báo; install.sh dùng đường binary trực tiếp nên không qua Gatekeeper.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
RID="${1:-osx-arm64}"
OUT="${2:-dist}"
VERSION="${3:-$(sed -n 's/.*<Version>\(.*\)<\/Version>.*/\1/p' "$HERE/../../macos/src/OrgInventoryAgent.MacOS/OrgInventoryAgent.MacOS.csproj" | head -n1)}"

case "$RID" in
    osx-x64|osx-arm64) ;;
    *) echo "RID không hỗ trợ: $RID (chỉ osx-x64 | osx-arm64)" >&2; exit 1 ;;
esac
[[ -n "$VERSION" ]] || { echo "Không xác định được VERSION" >&2; exit 1; }
command -v pkgbuild >/dev/null || { echo "Cần pkgbuild (chỉ có trên macOS)" >&2; exit 1; }

mkdir -p "$OUT"
PKGROOT="$OUT/pkgroot-$RID"
rm -rf "$PKGROOT"
mkdir -p "$PKGROOT/opt/orginventory" "$PKGROOT/Library/LaunchDaemons"

# AGENT_BIN=<path> → đóng gói binary build sẵn (Rust, build-macos-rust.sh) thay vì dotnet publish.
if [[ -n "${AGENT_BIN:-}" ]]; then
    install -m 0755 "$AGENT_BIN" "$PKGROOT/opt/orginventory/OrgInventoryAgent"
else
    dotnet publish "$HERE/../../macos/src/OrgInventoryAgent.MacOS/OrgInventoryAgent.MacOS.csproj" \
      -c Release -r "$RID" --self-contained true \
      -p:PublishSingleFile=true \
      -p:EnableCompressionInSingleFile=false \
      -o "$PKGROOT/opt/orginventory"
fi

printf '%s\n' "$VERSION" > "$PKGROOT/opt/orginventory/VERSION"
cp "$HERE/apply-update.sh" "$PKGROOT/opt/orginventory/"
chmod 0755 "$PKGROOT/opt/orginventory/apply-update.sh"
cp "$HERE"/com.orginventory.agent*.plist "$PKGROOT/Library/LaunchDaemons/"

SCRIPTS="$OUT/scripts-$RID"
rm -rf "$SCRIPTS"; mkdir -p "$SCRIPTS"
cp "$HERE/scripts/postinstall" "$SCRIPTS/postinstall"
chmod 0755 "$SCRIPTS/postinstall"

PKG="$OUT/orginventory-agent_${VERSION}_${RID}.pkg"
pkgbuild --root "$PKGROOT" \
  --identifier com.orginventory.agent \
  --version "$VERSION" \
  --scripts "$SCRIPTS" \
  --install-location / \
  "$PKG"
echo "Built $PKG (RID: $RID, unsigned)"
