#!/bin/bash
# =============================================================================
# Control Tower 一键构建并更新脚本
# =============================================================================
# 在 control-tower 目录下执行，自动构建 release 并更新到 /opt/ctsvc/
# 用法: ./scripts/build-and-update.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== Control Tower 构建并更新 ==="
echo "项目目录: $PROJECT_DIR"
echo ""

cd "$PROJECT_DIR"

# 1. 构建
echo "[1/4] 构建 release 版本..."
cargo build --release 2>&1 | tail -3

# 2. 更新
echo ""
echo "[2/4] 部署到 /opt/ctsvc/..."
systemctl stop ctsvc 2>/dev/null || true
sleep 2
cp target/release/ctsvc /opt/ctsvc/ctsvc
chmod +x /opt/ctsvc/ctsvc
echo "  已部署"

# 3. 启动
echo ""
echo "[3/4] 启动服务..."
systemctl start ctsvc
sleep 3

# 4. 验证
echo ""
echo "[4/4] 验证..."
STATUS=$(curl -s "http://localhost:8080/api/status" || echo "{}")
RUNNING=$(echo "$STATUS" | python3 -c "import sys,json; print(json.load(sys.stdin).get('data',{}).get('running',False))" 2>/dev/null || echo "error")
echo "  API status: running=$RUNNING"

if systemctl is-active --quiet ctsvc; then
    echo ""
    echo "=== 更新完成 ==="
else
    echo ""
    echo "=== 启动失败，查看日志 ==="
    journalctl -u ctsvc --no-pager -n 15
    exit 1
fi
