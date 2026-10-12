#!/bin/bash
# Build tarball portable cho OrgInventory Agent Linux — mọi distro có systemd.
#   ./build-tarball.sh [RID] [OUT]
#     RID: linux-x64 (mặc định) | linux-arm64 | linux-musl-x64 | linux-musl-arm64
#
# Tarball chứa:
#   orginventory-agent/opt/...      binary + apply-update.sh + VERSION
#   orginventory-agent/systemd/...  units (service + update.path/service)
#   orginventory-agent/install.sh   cài vào hệ thống (chạy bằng root)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
RID="${1:-linux-x64}"
OUT="${2:-dist}"

case "$RID" in
    linux-x64|linux-arm64|linux-musl-x64|linux-musl-arm64) ;;
    *) echo "RID không hỗ trợ: $RID" >&2; exit 1 ;;
esac

VERSION="$(sed -n 's/^Version: *//p' "$HERE/debian/control" | head -n1)"
[[ -n "$VERSION" ]] || { echo "Không đọc được Version từ debian/control" >&2; exit 1; }

mkdir -p "$OUT"
ROOT="$OUT/tarroot-$RID/orginventory-agent"
rm -rf "$OUT/tarroot-$RID"
mkdir -p "$ROOT/opt" "$ROOT/systemd"

dotnet publish "$HERE/../../linux/src/OrgInventoryAgent.Linux/OrgInventoryAgent.Linux.csproj" \
  -c Release -r "$RID" --self-contained true \
  -p:PublishSingleFile=true \
  -p:EnableCompressionInSingleFile=false \
  -o "$ROOT/opt" -p:ApplicationIcon=

printf '%s\n' "$VERSION" > "$ROOT/opt/VERSION"
cp "$HERE/apply-update.sh" "$ROOT/opt/apply-update.sh"
chmod 0755 "$ROOT/opt/apply-update.sh"
cp "$HERE"/systemd/orginventory-agent*.service "$HERE"/systemd/orginventory-agent*.path "$ROOT/systemd/"

# Script cài kèm trong tarball — chạy bằng root.
cat > "$ROOT/install.sh" <<'INSTEOF'
#!/bin/bash
# install.sh — cài OrgInventory Agent từ tarball (chạy bằng root).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
if [[ $EUID -ne 0 ]]; then echo "Cần root." >&2; exit 2; fi
if ! command -v systemctl >/dev/null; then
    echo "Distro không có systemd — dùng gói .apk (Alpine) hoặc cài thủ công." >&2; exit 3
fi

if ! getent group orginventory >/dev/null; then groupadd --system orginventory; fi
if ! getent passwd orginventory >/dev/null; then
    useradd --system --no-create-home --home /var/lib/orginventory -g orginventory orginventory
fi
mkdir -p /var/lib/orginventory /var/log/orginventory /etc/orginventory /run/orginventory
chown -R orginventory:orginventory /var/lib/orginventory /var/log/orginventory /run/orginventory
chmod 0750 /var/lib/orginventory /var/log/orginventory /run/orginventory

install -m 0755 "$HERE/opt/OrgInventoryAgent" /opt/orginventory/OrgInventoryAgent
install -m 0755 "$HERE/opt/apply-update.sh" /opt/orginventory/apply-update.sh
install -m 0644 "$HERE/opt/VERSION" /opt/orginventory/VERSION
install -m 0644 "$HERE"/systemd/orginventory-agent*.service "$HERE"/systemd/orginventory-agent*.path /etc/systemd/system/

systemctl daemon-reload
systemctl enable --now orginventory-agent-update.path
echo "Đã cài. Ghi /etc/orginventory/config.json (endpoints + enroll_token) rồi:"
echo "  systemctl enable --now orginventory-agent.service"
INSTEOF
chmod 0755 "$ROOT/install.sh"

TARBALL="$OUT/orginventory-agent_${VERSION}_${RID}.tar.gz"
tar -C "$OUT/tarroot-$RID" --owner=0 --group=0 -czf "$TARBALL" orginventory-agent
rm -rf "$OUT/tarroot-$RID"
echo "Built $TARBALL"
