#!/bin/bash
# =============================================================================
# apply-update.sh — áp dụng bản cập nhật agent đã stage.
#
# Agent (UpdateService, chạy user orginventory — không ghi được /opt) tải + verify
# SHA-256 binary mới vào /var/lib/orginventory/update/ rồi ghi marker
# update.pending. systemd unit orginventory-agent-update.path (PathExists) kích
# hoạt orginventory-agent-update.service → script này chạy bằng root:
#   re-verify SHA-256 → stop service → thay binary → ghi VERSION → start lại.
#
# Cũng có thể gọi tay (root) sau khi stage để áp dụng ngay.
# =============================================================================
set -u

DATA_DIR="/var/lib/orginventory"
STAGE="$DATA_DIR/update"
PENDING="$STAGE/update.pending"
BIN_DIR="/opt/orginventory"
BIN="$BIN_DIR/OrgInventoryAgent"
SERVICE="orginventory-agent.service"

if [[ ! -f "$PENDING" ]]; then
    # Cho phép gọi tay với arg = version (không cần marker).
    if [[ $# -lt 1 ]]; then
        echo "apply-update: không có update pending." >&2
        exit 0
    fi
fi

NEW_VER="$(head -n1 "$PENDING" 2>/dev/null || true)"
# Xóa marker TRƯỚC khi làm việc — path unit re-trigger ngay nếu marker còn
# tồn tại lúc oneshot kết thúc → vòng lặp vô hạn.
rm -f "$PENDING"

NEW_BIN="$STAGE/OrgInventoryAgent"
if [[ ! -f "$NEW_BIN" ]]; then
    echo "apply-update: thiếu binary staged ($NEW_BIN) — hủy." >&2
    exit 1
fi

# Re-verify SHA-256 (agent đã verify khi tải; kiểm tra lần nữa trước khi install).
# Fail-closed: staging dir agent ghi được — thiếu/không parse được sidecar thì
# KHÔNG được install binary chưa kiểm chứng vào /opt.
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
ACTUAL="$(sha256sum "$NEW_BIN" | awk '{print $1}')"
if [[ "$EXPECTED" != "$ACTUAL" ]]; then
    echo "apply-update: SHA256 mismatch ($EXPECTED != $ACTUAL) — hủy." >&2
    rm -rf "$STAGE"
    exit 1
fi

# Thay binary atomic: copy sang temp cùng filesystem (BIN_DIR), check từng bước,
# rồi mv — tránh ghi cụt khi đầy disk / lỗi giữa chừng mà vẫn start agent.
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

systemctl stop "$SERVICE" 2>/dev/null || true
if ! mv -f "$TMP_BIN" "$BIN"; then
    echo "apply-update: không thay được $BIN — hủy, khôi phục service bản cũ." >&2
    rm -f "$TMP_BIN"
    systemctl start "$SERVICE" 2>/dev/null || true
    exit 1
fi
if [[ -f "$STAGE/OrgInventoryAgent.version" ]]; then
    install -m 0644 "$STAGE/OrgInventoryAgent.version" "$BIN_DIR/VERSION"
elif [[ -n "$NEW_VER" ]]; then
    printf '%s\n' "$NEW_VER" > "$BIN_DIR/VERSION"
    chmod 0644 "$BIN_DIR/VERSION"
fi
rm -rf "$STAGE"
systemctl start "$SERVICE"
echo "apply-update: đã nâng cấp OrgInventoryAgent lên ${NEW_VER:-unknown}."
