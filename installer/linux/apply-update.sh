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
if [[ -f "$STAGE/OrgInventoryAgent.sha256" ]]; then
    EXPECTED="$(awk '{print $1}' "$STAGE/OrgInventoryAgent.sha256")"
    ACTUAL="$(sha256sum "$NEW_BIN" | awk '{print $1}')"
    if [[ "$EXPECTED" != "$ACTUAL" ]]; then
        echo "apply-update: SHA256 mismatch ($EXPECTED != $ACTUAL) — hủy." >&2
        rm -rf "$STAGE"
        exit 1
    fi
fi

systemctl stop "$SERVICE" 2>/dev/null || true
install -m 0755 "$NEW_BIN" "$BIN"
if [[ -f "$STAGE/OrgInventoryAgent.version" ]]; then
    install -m 0644 "$STAGE/OrgInventoryAgent.version" "$BIN_DIR/VERSION"
elif [[ -n "$NEW_VER" ]]; then
    printf '%s\n' "$NEW_VER" > "$BIN_DIR/VERSION"
    chmod 0644 "$BIN_DIR/VERSION"
fi
rm -rf "$STAGE"
systemctl start "$SERVICE"
echo "apply-update: đã nâng cấp OrgInventoryAgent lên ${NEW_VER:-unknown}."
