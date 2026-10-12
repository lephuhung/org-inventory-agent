#!/bin/sh
# =============================================================================
# apply-update.sh (Alpine/OpenRC) — áp dụng bản cập nhật agent đã stage.
# Giống bản systemd nhưng restart bằng rc-service. Chạy từ cron 15min
# (/etc/periodic/15min/orginventory-update) hoặc gọi tay bằng root.
# =============================================================================
set -u

DATA_DIR="/var/lib/orginventory"
STAGE="$DATA_DIR/update"
PENDING="$STAGE/update.pending"
BIN_DIR="/opt/orginventory"
BIN="$BIN_DIR/OrgInventoryAgent"
SERVICE="orginventory-agent"

if [ ! -f "$PENDING" ] && [ $# -lt 1 ]; then
    echo "apply-update: không có update pending." >&2
    exit 0
fi

NEW_VER="$(head -n1 "$PENDING" 2>/dev/null || true)"
rm -f "$PENDING"

NEW_BIN="$STAGE/OrgInventoryAgent"
if [ ! -f "$NEW_BIN" ]; then
    echo "apply-update: thiếu binary staged ($NEW_BIN) — hủy." >&2
    exit 1
fi

SHA_FILE="$STAGE/OrgInventoryAgent.sha256"
EXPECTED=""
if [ -f "$SHA_FILE" ]; then
    EXPECTED="$(awk 'NF{print $1; exit}' "$SHA_FILE" 2>/dev/null || true)"
fi
if ! echo "$EXPECTED" | grep -qE '^[0-9a-fA-F]{64}$'; then
    echo "apply-update: thiếu hoặc sai định dạng $SHA_FILE — hủy (fail-closed)." >&2
    rm -rf "$STAGE"
    exit 1
fi
ACTUAL="$(sha256sum "$NEW_BIN" | awk '{print $1}')"
if [ "$EXPECTED" != "$ACTUAL" ]; then
    echo "apply-update: SHA256 mismatch — hủy." >&2
    rm -rf "$STAGE"
    exit 1
fi

TMP_BIN="$(mktemp "$BIN_DIR/.OrgInventoryAgent.XXXXXX")" || exit 1
if ! cp "$NEW_BIN" "$TMP_BIN"; then
    echo "apply-update: copy binary thất bại — hủy, giữ bản cũ." >&2
    rm -f "$TMP_BIN"
    exit 1
fi
chmod 0755 "$TMP_BIN" || true

rc-service "$SERVICE" stop 2>/dev/null || true
if ! mv -f "$TMP_BIN" "$BIN"; then
    echo "apply-update: không thay được $BIN — hủy." >&2
    rm -f "$TMP_BIN"
    rc-service "$SERVICE" start 2>/dev/null || true
    exit 1
fi
if [ -f "$STAGE/OrgInventoryAgent.version" ]; then
    cp "$STAGE/OrgInventoryAgent.version" "$BIN_DIR/VERSION"
    chmod 0644 "$BIN_DIR/VERSION"
elif [ -n "$NEW_VER" ]; then
    printf '%s\n' "$NEW_VER" > "$BIN_DIR/VERSION"
    chmod 0644 "$BIN_DIR/VERSION"
fi
rm -rf "$STAGE"
rc-service "$SERVICE" start 2>/dev/null || true
echo "apply-update: đã nâng cấp OrgInventoryAgent lên ${NEW_VER:-unknown}."
