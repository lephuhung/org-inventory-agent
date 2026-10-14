#!/bin/bash
# Build gói pacman (.pkg.tar.zst) cho OrgInventory Agent — Arch/Manjaro.
#   ./build-pacman.sh [RID] [OUT]
#     RID: linux-x64 (mặc định) | linux-arm64
#
# Không cần Arch host: .pkg.tar.zst chỉ là tar.zst chứa .PKGINFO + .INSTALL +
# .MTREE + payload. Cần zstd (apt install zstd) hoặc bsdtar.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
RID="${1:-linux-x64}"
OUT="${2:-dist}"

case "$RID" in
    linux-x64)   PKG_ARCH="x86_64" ;;
    linux-arm64) PKG_ARCH="aarch64" ;;
    *) echo "RID không hỗ trợ: $RID (chỉ linux-x64 | linux-arm64)" >&2; exit 1 ;;
esac
command -v zstd >/dev/null || { echo "Cần zstd (apt-get install -y zstd)" >&2; exit 1; }

VERSION="$(sed -n 's/^Version: *//p' "$HERE/debian/control" | head -n1)"
[[ -n "$VERSION" ]] || { echo "Không đọc được Version từ debian/control" >&2; exit 1; }

mkdir -p "$OUT"
PKGROOT="$OUT/pacroot-$RID"
rm -rf "$PKGROOT"
# Arch: units vào /usr/lib/systemd/system (khác /etc của install script thủ công).
mkdir -p "$PKGROOT/opt/orginventory" "$PKGROOT/etc/orginventory" "$PKGROOT/usr/lib/systemd/system"

# AGENT_BIN=<path> → đóng gói binary build sẵn (Rust) thay vì dotnet publish.
if [[ -n "${AGENT_BIN:-}" ]]; then
    install -m 0755 "$AGENT_BIN" "$PKGROOT/opt/orginventory/OrgInventoryAgent"
else
    dotnet publish "$HERE/../../linux/src/OrgInventoryAgent.Linux/OrgInventoryAgent.Linux.csproj" \
      -c Release -r "$RID" --self-contained true \
      -p:PublishSingleFile=true \
      -p:EnableCompressionInSingleFile=false \
      -o "$PKGROOT/opt/orginventory" -p:ApplicationIcon=
fi

printf '%s\n' "$VERSION" > "$PKGROOT/opt/orginventory/VERSION"
cp "$HERE/apply-update.sh" "$PKGROOT/opt/orginventory/"
chmod 0755 "$PKGROOT/opt/orginventory/apply-update.sh"
cp "$HERE"/systemd/orginventory-agent*.service "$HERE"/systemd/orginventory-agent*.path \
    "$PKGROOT/usr/lib/systemd/system/"

# .INSTALL scriptlet — tương đương debian/postinst + prerm.
cat > "$PKGROOT/.INSTALL" <<'INSTALLEOF'
post_install() {
    getent group orginventory >/dev/null || groupadd --system orginventory
    getent passwd orginventory >/dev/null || \
        useradd --system --no-create-home --home /var/lib/orginventory -g orginventory orginventory
    mkdir -p /var/lib/orginventory /var/log/orginventory /run/orginventory
    chown -R orginventory:orginventory /var/lib/orginventory /var/log/orginventory /run/orginventory
    chmod 0750 /var/lib/orginventory /var/log/orginventory /run/orginventory
    systemctl daemon-reload
    # Auto-updater: path unit watch marker agent stage → apply-update.sh (root).
    systemctl enable --now orginventory-agent-update.path 2>/dev/null || true
    # Service chính KHÔNG start tự động — cần enroll token trước.
}

post_upgrade() {
    post_install
}

pre_remove() {
    systemctl stop orginventory-agent.service orginventory-agent-update.path 2>/dev/null || true
    systemctl disable orginventory-agent.service orginventory-agent-update.path 2>/dev/null || true
    systemctl daemon-reload
}
INSTALLEOF

INSTALLED_SIZE="$(du -sb "$PKGROOT" | awk '{print $1}')"
cat > "$PKGROOT/.PKGINFO" <<EOF
pkgname = orginventory-agent
pkgver = ${VERSION}-1
pkgdesc = OrgInventory Agent — IT Asset Inventory for Linux
url = https://github.com/lephuhung/org-inventory-agent
builddate = $(date +%s)
packager = OrgInventory Team
size = ${INSTALLED_SIZE}
arch = ${PKG_ARCH}
license = custom:proprietary
depend = systemd
EOF

# .MTREE — map file → sha256 để pacman -Qkk verify được.
MTREE="$PKGROOT/.MTREE"
{
    echo "#mtree"
    echo "/set type=file uid=0 gid=0 mode=644"
    (cd "$PKGROOT" && find opt etc usr -type f) | sort | while read -r f; do
        mode="$(stat -c '%a' "$PKGROOT/$f")"
        size="$(stat -c '%s' "$PKGROOT/$f")"
        sha="$(sha256sum "$PKGROOT/$f" | awk '{print $1}')"
        echo "./$f mode=$mode size=$size sha256digest=$sha"
    done
} > "$MTREE"

PKG="$OUT/orginventory-agent-${VERSION}-1-${PKG_ARCH}.pkg.tar.zst"
tar -C "$PKGROOT" --owner=0 --group=0 -cf - .PKGINFO .INSTALL .MTREE opt etc usr \
    | zstd -19 -q -o "$PKG"
rm -rf "$PKGROOT"
echo "Built $PKG (arch: $PKG_ARCH, RID: $RID)"
