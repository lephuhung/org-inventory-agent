#!/bin/bash
# =============================================================================
# apply-update.sh (macOS) — áp dụng bản cập nhật agent đã stage.
#
# Agent (UpdateService) tải + verify SHA-256 binary mới vào
# /var/db/orginventory/update/ rồi ghi marker update.pending. LaunchDaemon
# com.orginventory.agent.update (WatchPaths) kích hoạt script này (root):
#   re-verify SHA-256 → thay binary → kickstart lại daemon.
#
# Cũng có thể gọi tay (root) sau khi stage để áp dụng ngay.
# =============================================================================
set -u

DATA_DIR="/var/db/orginventory"
STAGE="$DATA_DIR/update"
PENDING="$STAGE/update.pending"
BIN_DIR="/opt/orginventory"
BIN="$BIN_DIR/OrgInventoryAgent"
LABEL="com.orginventory.agent"

if [[ ! -f "$PENDING" ]]; then
    # Cho phép gọi tay với arg = version (không cần marker).
    if [[ $# -lt 1 ]]; then
        echo "apply-update: không có update pending." >&2
        exit 0
    fi
fi

NEW_VER="$(head -n1 "$PENDING" 2>/dev/null || true)"
rm -f "$PENDING"

NEW_BIN="$STAGE/OrgInventoryAgent"
if [[ ! -f "$NEW_BIN" ]]; then
    echo "apply-update: thiếu binary staged ($NEW_BIN) — hủy." >&2
    exit 1
fi

# Re-verify SHA-256 — fail-closed giống bản Linux: thiếu/sai sidecar thì hủy.
SHA_FILE="$STAGE/OrgInventoryAgent.sha256"
EXPECTED=""
if [[ -f "$SHA_FILE" ]]; then
    EXPECTED="$(awk 'NF{print $1; exit}' "$SHA_FILE" 2>/dev/null || true)"
fi
if [[ ! "$EXPECTED" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "apply-update: thiếu hoặc sai định dạng $SHA_FILE — hủy (fail-closed)." >&2
    rm -rf "$STAGE"
    exit 1
fi
# macOS không có sha256sum — dùng shasum -a 256.
ACTUAL="$(shasum -a 256 "$NEW_BIN" | awk '{print $1}')"
if [[ "$EXPECTED" != "$ACTUAL" ]]; then
    echo "apply-update: SHA256 mismatch ($EXPECTED != $ACTUAL) — hủy." >&2
    rm -rf "$STAGE"
    exit 1
fi

# Thay binary atomic: copy sang temp cùng filesystem rồi mv.
TMP_BIN="$(mktemp "$BIN_DIR/.OrgInventoryAgent.XXXXXX")" || {
    echo "apply-update: không tạo được temp trong $BIN_DIR — hủy." >&2
    exit 1
}
if ! cp "$NEW_BIN" "$TMP_BIN"; then
    echo "apply-update: copy binary thất bại (đầy disk?) — hủy, giữ bản cũ." >&2
    rm -f "$TMP_BIN"
    exit 1
fi
chmod 0755 "$TMP_BIN" || true

if ! mv -f "$TMP_BIN" "$BIN"; then
    echo "apply-update: không thay được $BIN — hủy." >&2
    rm -f "$TMP_BIN"
    exit 1
fi
if [[ -f "$STAGE/OrgInventoryAgent.version" ]]; then
    install -m 0644 "$STAGE/OrgInventoryAgent.version" "$BIN_DIR/VERSION" 2>/dev/null \
        || cp "$STAGE/OrgInventoryAgent.version" "$BIN_DIR/VERSION"
elif [[ -n "$NEW_VER" ]]; then
    printf '%s\n' "$NEW_VER" > "$BIN_DIR/VERSION"
    chmod 0644 "$BIN_DIR/VERSION"
fi
rm -rf "$STAGE"

# kickstart -k = stop + start lại daemon (tương đương systemctl restart).
launchctl kickstart -k "system/$LABEL" 2>/dev/null || {
    # Fallback cho macOS cũ: bootout + bootstrap.
    launchctl bootout "system/$LABEL" 2>/dev/null || true
    launchctl bootstrap system "/Library/LaunchDaemons/$LABEL.plist" 2>/dev/null || true
}
echo "apply-update: đã nâng cấp OrgInventoryAgent lên ${NEW_VER:-unknown}."
