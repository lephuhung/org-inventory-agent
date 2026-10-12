#!/bin/bash
# =============================================================================
# install.sh — cài OrgInventory Agent macOS (curl | sudo bash).
#   curl -fsSL https://<host>/i/<token> | sudo bash
# hoặc chạy trực tiếp với biến môi trường:
#   sudo ORGINV_TOKEN=<token> ORGINV_HOST=<host> ./install.sh
#
# Contract giữ nguyên bản Linux:
#   - Binary + apply-update.sh + VERSION → /opt/orginventory
#   - Data dir (staging update, state.json) → /var/db/orginventory
#   - Bootstrap config (enroll_token + endpoints) → /etc/orginventory/config.json
#   - LaunchDaemon com.orginventory.agent + watcher com.orginventory.agent.update
#   - KHÔNG bao giờ mất identity trừ khi REENROLL=1 — config cũ được merge
#     (giữ key lạ) thay vì ghi đè khi có python3; nếu không merge được thì giữ
#     nguyên config cũ (agent vẫn hoạt động với token cũ).
# =============================================================================
set -euo pipefail

TOKEN="${ORGINV_TOKEN:-}"
HOST="${ORGINV_HOST:-}"
REENROLL="${REENROLL:-0}"
BASE_URL="${ORGINV_BASE_URL:-}"

if [[ $EUID -ne 0 ]]; then
    echo "Cần quyền root. Chạy lại với sudo." >&2
    exit 2
fi

BIN_DIR="/opt/orginventory"
DATA_DIR="/var/db/orginventory"
LOG_DIR="/var/log/orginventory"
CONF_DIR="/etc/orginventory"
CONF="$CONF_DIR/config.json"
LAUNCHD="/Library/LaunchDaemons"

# --- RID theo kiến trúc -------------------------------------------------------
ARCH="$(uname -m)"
case "$ARCH" in
    arm64)  RID="osx-arm64" ;;
    x86_64) RID="osx-x64" ;;
    *) echo "Không hỗ trợ kiến trúc $ARCH" >&2; exit 3 ;;
esac

# --- Nguồn binary -------------------------------------------------------------
# Ưu tiên tải từ backend (ORGINV_HOST + ORGINV_TOKEN → download endpoint);
# fallback ORGINV_BASE_URL trỏ trực tiếp (file:// hoặc https://) cho offline.
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [[ -n "$BASE_URL" ]]; then
    BIN_URL="${BASE_URL%/}/OrgInventoryAgent-${RID}"
    SHA_URL="${BIN_URL}.sha256"
    echo "Đang tải $BIN_URL ..."
    curl -fsSL -o "$TMP/OrgInventoryAgent" "$BIN_URL"
    if curl -fsSL -o "$TMP/OrgInventoryAgent.sha256" "$SHA_URL" 2>/dev/null; then
        EXPECTED="$(awk '{print $1}' "$TMP/OrgInventoryAgent.sha256")"
        ACTUAL="$(shasum -a 256 "$TMP/OrgInventoryAgent" | awk '{print $1}')"
        [[ "$EXPECTED" == "$ACTUAL" ]] || { echo "SHA256 mismatch" >&2; exit 4; }
    else
        echo "Cảnh báo: không có .sha256 — bỏ qua verify (offline mode)." >&2
    fi
elif [[ -n "$TOKEN" && -n "$HOST" ]]; then
    DL="https://${HOST}/download/macos/${TOKEN}"
    echo "Đang tải ${DL}/OrgInventoryAgent-${RID} ..."
    curl -fsSL -o "$TMP/OrgInventoryAgent" "${DL}/OrgInventoryAgent-${RID}"
    curl -fsSL -o "$TMP/OrgInventoryAgent.sha256" "${DL}/OrgInventoryAgent-${RID}.sha256"
    EXPECTED="$(awk '{print $1}' "$TMP/OrgInventoryAgent.sha256")"
    ACTUAL="$(shasum -a 256 "$TMP/OrgInventoryAgent" | awk '{print $1}')"
    [[ "$EXPECTED" == "$ACTUAL" ]] || { echo "SHA256 mismatch" >&2; exit 4; }
else
    echo "Thiếu nguồn binary. Cần một trong hai:" >&2
    echo "  ORGINV_TOKEN + ORGINV_HOST (tải từ backend), hoặc" >&2
    echo "  ORGINV_BASE_URL trỏ tới thư mục chứa OrgInventoryAgent-${RID}[.sha256]" >&2
    exit 1
fi

# --- Cài file -----------------------------------------------------------------
mkdir -p "$BIN_DIR" "$DATA_DIR" "$LOG_DIR" "$CONF_DIR"
install -m 0755 "$TMP/OrgInventoryAgent" "$BIN_DIR/OrgInventoryAgent"

HERE="$(cd "$(dirname "$0")" && pwd)"
if [[ -f "$HERE/apply-update.sh" ]]; then
    install -m 0755 "$HERE/apply-update.sh" "$BIN_DIR/apply-update.sh"
else
    # Đường curl|bash: lấy apply-update-macos.sh cùng nguồn với binary
    # (release asset đặt tên apply-update-macos.sh để khỏi lẫn bản Linux).
    if [[ -n "$BASE_URL" ]]; then
        curl -fsSL -o "$BIN_DIR/apply-update.sh" "${BASE_URL%/}/apply-update-macos.sh" || true
    elif [[ -n "$TOKEN" && -n "$HOST" ]]; then
        curl -fsSL -o "$BIN_DIR/apply-update.sh" "${DL}/apply-update-macos.sh" || true
    fi
    [[ -f "$BIN_DIR/apply-update.sh" ]] && chmod 0755 "$BIN_DIR/apply-update.sh"
fi

# VERSION sidecar (để backend/manifest so sánh).
if [[ -n "$BASE_URL" ]]; then
    curl -fsSL -o "$BIN_DIR/VERSION" "${BASE_URL%/}/OrgInventoryAgent-${RID}.version" 2>/dev/null || true
elif [[ -n "$TOKEN" && -n "$HOST" ]]; then
    curl -fsSL -o "$BIN_DIR/VERSION" "${DL}/OrgInventoryAgent-${RID}.version" 2>/dev/null || true
fi

chmod 0750 "$DATA_DIR" "$LOG_DIR"

# --- LaunchDaemon plists --------------------------------------------------------
install_plist() {
    local name="$1"
    if [[ -f "$HERE/$name" ]]; then
        install -m 0644 "$HERE/$name" "$LAUNCHD/$name"
    else
        local url=""
        [[ -n "$BASE_URL" ]] && url="${BASE_URL%/}/$name"
        [[ -z "$url" && -n "${DL:-}" ]] && url="${DL}/$name"
        [[ -n "$url" ]] && curl -fsSL -o "$LAUNCHD/$name" "$url"
    fi
    [[ -f "$LAUNCHD/$name" ]] && chmod 0644 "$LAUNCHD/$name" && chown root:wheel "$LAUNCHD/$name"
}
install_plist com.orginventory.agent.plist
install_plist com.orginventory.agent.update.plist

# --- Config bootstrap (merge giữ identity) --------------------------------------
if [[ "$REENROLL" == "1" ]]; then
    rm -f "$CONF" "$DATA_DIR/state.json" "$DATA_DIR"/client-*.pem 2>/dev/null || true
fi

if [[ -n "$TOKEN" && -n "$HOST" ]]; then
    if [[ ! -f "$CONF" ]]; then
        cat > "$CONF" <<EOF
{
  "endpoints": ["https://${HOST}"],
  "enroll_token": "${TOKEN}",
  "data_dir": "${DATA_DIR}"
}
EOF
    elif command -v python3 >/dev/null; then
        # Merge: cập nhật endpoints + enroll_token, GIỮ mọi key khác (identity).
        python3 - "$CONF" "$HOST" "$TOKEN" "$DATA_DIR" <<'PYEOF'
import json, sys
path, host, token, data_dir = sys.argv[1:5]
try:
    with open(path) as f:
        cfg = json.load(f)
except Exception:
    cfg = {}
cfg["endpoints"] = ["https://" + host]
cfg["enroll_token"] = token
cfg.setdefault("data_dir", data_dir)
with open(path, "w") as f:
    json.dump(cfg, f, indent=2)
PYEOF
    else
        echo "Giữ nguyên config cũ (không có python3 để merge)." >&2
    fi
    chmod 0640 "$CONF" 2>/dev/null || true
fi

# --- Nạp daemons ---------------------------------------------------------------
launchctl bootstrap system "$LAUNCHD/com.orginventory.agent.update.plist" 2>/dev/null \
    || launchctl load "$LAUNCHD/com.orginventory.agent.update.plist" 2>/dev/null || true

if [[ -f "$CONF" ]]; then
    launchctl bootout system/com.orginventory.agent 2>/dev/null || true
    launchctl bootstrap system "$LAUNCHD/com.orginventory.agent.plist" 2>/dev/null \
        || launchctl load "$LAUNCHD/com.orginventory.agent.plist"
    echo "✔ Cài đặt thành công. Agent đang enroll (log: $LOG_DIR/agent.stderr.log)..."
else
    echo "✔ Đã cài binary. Ghi $CONF (endpoints + enroll_token) rồi chạy:" >&2
    echo "  sudo launchctl bootstrap system $LAUNCHD/com.orginventory.agent.plist" >&2
fi
