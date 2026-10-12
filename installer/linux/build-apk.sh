#!/bin/bash
# Build gói Alpine .apk (unsigned) cho OrgInventory Agent.
#   ./build-apk.sh [RID] [OUT]
#     RID: linux-musl-x64 (mặc định) | linux-musl-arm64
#
# .apk v2 = control.tar.gz + data.tar.gz nối nhau (unsigned — cài bằng
# `apk add --allow-untrusted`). Alpine không có systemd → OpenRC init script +
# cron 15min cho auto-update.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
RID="${1:-linux-musl-x64}"
OUT="${2:-dist}"

case "$RID" in
    linux-musl-x64)   APK_ARCH="x86_64" ;;
    linux-musl-arm64) APK_ARCH="aarch64" ;;
    *) echo "RID không hỗ trợ: $RID (chỉ linux-musl-x64 | linux-musl-arm64)" >&2; exit 1 ;;
esac

VERSION="$(sed -n 's/^Version: *//p' "$HERE/debian/control" | head -n1)"
[[ -n "$VERSION" ]] || { echo "Không đọc được Version từ debian/control" >&2; exit 1; }

mkdir -p "$OUT"
WORK="$OUT/apkwork-$RID"
rm -rf "$WORK"
DATA="$WORK/data"
CTRL="$WORK/control"
mkdir -p "$DATA/opt/orginventory" "$DATA/etc/orginventory" "$DATA/etc/init.d" \
         "$DATA/etc/periodic/15min" "$CTRL"

dotnet publish "$HERE/../../linux/src/OrgInventoryAgent.Linux/OrgInventoryAgent.Linux.csproj" \
  -c Release -r "$RID" --self-contained true \
  -p:PublishSingleFile=true \
  -p:EnableCompressionInSingleFile=false \
  -o "$DATA/opt/orginventory" -p:ApplicationIcon=

printf '%s\n' "$VERSION" > "$DATA/opt/orginventory/VERSION"
cp "$HERE/alpine/apply-update.sh" "$DATA/opt/orginventory/apply-update.sh"
chmod 0755 "$DATA/opt/orginventory/apply-update.sh"
install -m 0755 "$HERE/alpine/orginventory-agent.initd" "$DATA/etc/init.d/orginventory-agent"
install -m 0755 "$HERE/alpine/orginventory-update.cron" "$DATA/etc/periodic/15min/orginventory-update"

# --- control.tar.gz: .PKGINFO + scriptlets ------------------------------------
INSTALLED_SIZE="$(du -sb "$DATA" | awk '{print $1}')"
cat > "$CTRL/.PKGINFO" <<EOF
pkgname = orginventory-agent
pkgver = ${VERSION}-r0
pkgdesc = OrgInventory Agent - IT Asset Inventory for Linux (musl)
url = https://github.com/lephuhung/org-inventory-agent
size = ${INSTALLED_SIZE}
arch = ${APK_ARCH}
origin = orginventory-agent
maintainer = OrgInventory Team
license = proprietary
commit = manual-build
EOF

# BusyBox user: addgroup/adduser (không phải groupadd/useradd).
cat > "$CTRL/.pre-install" <<'EOF'
#!/bin/sh
addgroup -S orginventory 2>/dev/null
adduser -S -D -H -h /var/lib/orginventory -G orginventory orginventory 2>/dev/null
exit 0
EOF

cat > "$CTRL/.post-install" <<'EOF'
#!/bin/sh
mkdir -p /var/lib/orginventory /var/log/orginventory
chown -R orginventory:orginventory /var/lib/orginventory /var/log/orginventory
chmod 0750 /var/lib/orginventory /var/log/orginventory
rc-update add orginventory-agent default 2>/dev/null || true
# Service chính KHÔNG start tự động — cần enroll token trước.
exit 0
EOF

cat > "$CTRL/.pre-deinstall" <<'EOF'
#!/bin/sh
rc-service orginventory-agent stop 2>/dev/null || true
rc-update del orginventory-agent default 2>/dev/null || true
exit 0
EOF

tar -C "$CTRL" --owner=0 --group=0 -czf "$WORK/control.tar.gz" .
tar -C "$DATA" --owner=0 --group=0 -czf "$WORK/data.tar.gz" .

APK="$OUT/orginventory-agent-${VERSION}-r0-${APK_ARCH}.apk"
cat "$WORK/control.tar.gz" "$WORK/data.tar.gz" > "$APK"
rm -rf "$WORK"
echo "Built $APK (arch: $APK_ARCH, RID: $RID, unsigned — cài bằng: apk add --allow-untrusted $APK)"
