#!/bin/bash
# =============================================================================
# Control Tower 更新脚本
# =============================================================================
# 用法: ./update-ctsvc.sh <新二进制路径>
# 示例: ./update-ctsvc.sh /home/jason/tui-clash-verge/control-tower/target/release/ctsvc
#
# 或直接执行 (自动找当前目录的 release 二进制):
#   ./update-ctsvc.sh
# =============================================================================

set -euo pipefail

CTCTL_DIR="/opt/ctsvc"
BINARY_NAME="ctsvc"
SERVICE_NAME="ctsvc"

# 解析参数
SOURCE_BIN="${1:-}"
if [[ -z "$SOURCE_BIN" ]]; then
    # 自动查找当前目录的 release 二进制
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    SOURCE_BIN="$SCRIPT_DIR/target/release/$BINARY_NAME"
fi

# 验证源文件
if [[ ! -f "$SOURCE_BIN" ]]; then
    echo "ERROR: 源二进制不存在: $SOURCE_BIN"
    echo "用法: $0 <新二进制路径>"
    exit 1
fi

echo "=== Control Tower 更新脚本 ==="
echo "源文件: $SOURCE_BIN"
echo "目标目录: $CTCTL_DIR"
echo ""

# 1. 检查服务状态
echo "[1/6] 检查服务状态..."
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    RUNNING=true
    echo "  服务当前运行中"
else
    RUNNING=false
    echo "  服务当前未运行"
fi

# 2. 备份
echo "[2/6] 备份当前二进制..."
BACKUP_BIN="$CTCTL_DIR/${BINARY_NAME}.backup.$(date +%Y%m%d_%H%M%S)"
cp "$CTCTL_DIR/$BINARY_NAME" "$BACKUP_BIN"
echo "  备份: $BACKUP_BIN"

# 3. 停止服务
echo "[3/6] 停止服务..."
systemctl stop "$SERVICE_NAME" 2>/dev/null || true
sleep 2

# 确认已停止
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "  WARNING: 服务未能停止，强制终止..."
    pkill -f "$CTCTL_DIR/$BINARY_NAME" 2>/dev/null || true
    sleep 1
fi
echo "  已停止"

# 4. 部署新二进制
echo "[4/6] 部署新二进制..."
cp "$SOURCE_BIN" "$CTCTL_DIR/$BINARY_NAME"
chmod +x "$CTCTL_DIR/$BINARY_NAME"
NEW_SIZE=$(stat -c%s "$CTCTL_DIR/$BINARY_NAME")
echo "  已部署: $CTCTL_DIR/$BINARY_NAME ($NEW_SIZE bytes)"

# 5. 启动服务
echo "[5/6] 启动服务..."
systemctl start "$SERVICE_NAME"
sleep 3

# 6. 验证
echo "[6/6] 验证部署..."
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "  服务启动成功 ✓"

    # 检查 Mihomo 子进程
    if pgrep -f "$CTCTL_DIR/mihomo" > /dev/null; then
        echo "  Mihomo 子进程运行中 ✓"
    else
        echo "  WARNING: Mihomo 子进程未检测到"
    fi

    # 检查 API
    API_STATUS=$(curl -s "http://localhost:8080/api/status" 2>/dev/null || echo "{}")
    RUNNING=$(echo "$API_STATUS" | python3 -c "import sys,json; print(json.load(sys.stdin).get('data',{}).get('running',False))" 2>/dev/null || echo "unknown")
    STATE=$(echo "$API_STATUS" | python3 -c "import sys,json; print(json.load(sys.stdin).get('data',{}).get('state','unknown'))" 2>/dev/null || echo "unknown")

    echo "  API /api/status: running=$RUNNING, state=$STATE"

    if [[ "$RUNNING" == "True" ]]; then
        echo ""
        echo "=== 更新成功 ==="
    else
        echo ""
        echo "=== 服务已启动但 Mihomo 未运行 ==="
        echo "可能需要手动启动: curl -X POST http://localhost:8080/api/service/start"
    fi
else
    echo "  ERROR: 服务启动失败!"
    echo ""
    echo "查看日志:"
    journalctl -u "$SERVICE_NAME" --no-pager -n 20
    exit 1
fi

# 清理旧备份 (保留最近 3 个)
echo ""
echo "清理备份文件 (保留最近 3 个)..."
BACKUPS=($(ls -t "$CTCTL_DIR"/${BINARY_NAME}.backup.* 2>/dev/null))
if [[ ${#BACKUPS[@]} -gt 3 ]]; then
    for old_backup in "${BACKUPS[@]:3}"; do
        rm -f "$old_backup" && echo "  删除: $old_backup"
    done
fi
echo "完成"
