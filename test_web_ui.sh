#!/bin/bash
# Web UI Automated Test Script
# Tests all API endpoints and Web UI functionality

BASE_URL="http://localhost:8080"

echo "============================================"
echo "Control Tower Web UI - Automated Test"
echo "============================================"
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
fail() { echo -e "${RED}[FAIL]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
info() { echo -e "[INFO] $1"; }

# Helper: HTTP request via curl (TCP)
api() {
    local method=$1
    local path=$2
    local body=$3
    if [ -n "$body" ]; then
        curl -s -X $method -H "Content-Type: application/json" -d "$body" "$BASE_URL$path"
    else
        curl -s -X $method "$BASE_URL$path"
    fi
}

# Helper: Check JSON response code
check_code() {
    local resp=$1
    echo "$resp" | grep -q '"code":0' && return 0 || return 1
}

# ============================================
# PAGE 1: DASHBOARD
# ============================================
echo "=========================================="
echo "PAGE 1: DASHBOARD"
echo "=========================================="

echo ""
echo "--- /api/status ---"
resp=$(api GET "/api/status")
echo "$resp"
if check_code "$resp"; then
    pass "status endpoint works"
else
    fail "status endpoint failed"
fi

echo ""
echo "--- /api/mode ---"
resp=$(api GET "/api/mode")
echo "$resp"
if check_code "$resp"; then
    pass "mode endpoint works"
else
    fail "mode endpoint failed"
fi

echo ""
echo "--- POST /api/service/start ---"
resp=$(api POST "/api/service/start" "{}")
echo "$resp"
if check_code "$resp"; then
    pass "service start works"
else
    fail "service start failed"
fi
sleep 1

echo ""
echo "--- POST /api/mode (set rule) ---"
resp=$(api POST "/api/mode" '{"mode":"rule"}')
echo "$resp"
if check_code "$resp"; then
    pass "set mode to rule works"
else
    fail "set mode failed"
fi

echo ""
echo "--- POST /api/mode (set global) ---"
resp=$(api POST "/api/mode" '{"mode":"global"}')
echo "$resp"
if check_code "$resp"; then
    pass "set mode to global works"
else
    fail "set mode to global failed"
fi

echo ""
echo "--- POST /api/service/stop ---"
resp=$(api POST "/api/service/stop" "{}")
echo "$resp"
if check_code "$resp"; then
    pass "service stop works"
else
    fail "service stop failed"
fi

# ============================================
# PAGE 2: PROXIES
# ============================================
echo ""
echo "=========================================="
echo "PAGE 2: PROXIES"
echo "=========================================="

echo ""
echo "--- GET /api/proxies ---"
resp=$(api GET "/api/proxies")
echo "$resp" | head -c 500
echo "..."
if echo "$resp" | grep -q '"code":0'; then
    pass "proxies endpoint works"
    # Count proxies
    count=$(echo "$resp" | grep -o '"name"' | wc -l)
    info "Found $count proxy entries"
else
    fail "proxies endpoint failed"
fi

echo ""
echo "--- GET /api/proxies/{name}/delay ---"
# First get a proxy name
proxy_name=$(api GET "/api/proxies" | python3 -c "import sys,json; d=json.load(sys.stdin); keys=[k for k in d.get('data',{}).get('proxies',{}).keys() if k not in ['GLOBAL','DIRECT','REJECT','FALLBACK']]; print(keys[0] if keys else '')" 2>/dev/null)
if [ -n "$proxy_name" ]; then
    encoded=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$proxy_name'))")
    resp=$(api GET "/api/proxies/$encoded/delay?timeout=5000")
    echo "$resp"
    if check_code "$resp"; then
        pass "proxy delay works for '$proxy_name'"
    else
        warn "proxy delay failed (may be expected if proxy not reachable)"
    fi
else
    warn "No proxy names found to test delay"
fi

echo ""
echo "--- POST /api/proxies/select ---"
# Get GLOBAL selector name first
resp=$(api GET "/api/proxies")
echo "$resp" | head -c 300
echo "..."
if echo "$resp" | grep -q '"code":0'; then
    pass "can read proxies to select from"
else
    fail "cannot read proxies"
fi

# ============================================
# PAGE 3: PROFILES
# ============================================
echo ""
echo "=========================================="
echo "PAGE 3: PROFILES"
echo "=========================================="

echo ""
echo "--- GET /api/profiles ---"
resp=$(api GET "/api/profiles")
echo "$resp" | head -c 500
echo "..."
if check_code "$resp"; then
    pass "profiles endpoint works"
else
    fail "profiles endpoint failed"
fi

echo ""
echo "--- POST /api/profiles (add) ---"
# Test with an invalid URL (should fail but shows the endpoint works)
resp=$(api POST "/api/profiles" '{"url":"http://10.255.255.1/test.yaml","name":"test"}')
echo "$resp"
if check_code "$resp"; then
    pass "add profile accepted (network timeout expected)"
else
    # Could be timeout or actual error
    msg=$(echo "$resp" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('message','')[:100])" 2>/dev/null)
    info "add profile response: $msg"
fi

echo ""
echo "--- GET /api/config ---"
resp=$(api GET "/api/config")
echo "$resp" | head -c 500
echo "..."
if check_code "$resp"; then
    pass "config endpoint works"
else
    fail "config endpoint failed"
fi

# ============================================
# PAGE 4: RULES
# ============================================
echo ""
echo "=========================================="
echo "PAGE 4: RULES"
echo "=========================================="

echo ""
echo "--- GET /api/config (rules are part of config) ---"
resp=$(api GET "/api/config")
echo "$resp" | head -c 300
echo "..."
if check_code "$resp"; then
    rules_count=$(echo "$resp" | python3 -c "import sys,json; d=json.load(sys.stdin); r=d.get('data',{}).get('rules',[]); print(len(r))" 2>/dev/null)
    info "Found $rules_count rules in config"
    pass "config with rules accessible"
else
    fail "config endpoint failed"
fi

echo ""
echo "--- POST /api/rules (expect unimplemented) ---"
resp=$(api POST "/api/rules" '{"type":"DOMAIN","value":"example.com","proxy":"direct"}')
echo "$resp"
if echo "$resp" | grep -q '"code":0\|not implemented\|not support\|error'; then
    info "rules endpoint response received"
else
    warn "unexpected rules response"
fi

# ============================================
# PAGE 5: CONNECTIONS
# ============================================
echo ""
echo "=========================================="
echo "PAGE 5: CONNECTIONS"
echo "=========================================="

echo ""
echo "--- GET /api/connections ---"
resp=$(api GET "/api/connections")
echo "$resp" | head -c 500
echo "..."
if check_code "$resp"; then
    pass "connections endpoint works"
    conn_count=$(echo "$resp" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('data',{}).get('connections',[])))" 2>/dev/null)
    info "Active connections: $conn_count"
else
    fail "connections endpoint failed"
fi

echo ""
echo "--- DELETE /api/connections/{id} (expect fail - no id) ---"
resp=$(api DELETE "/api/connections/nonexistent-id")
echo "$resp"
if echo "$resp" | grep -q '"code":0\|not found\|error\|No such'; then
    info "connection delete handled gracefully"
else
    warn "unexpected delete response"
fi

# ============================================
# PAGE 6: SETTINGS
# ============================================
echo ""
echo "=========================================="
echo "PAGE 6: SETTINGS"
echo "=========================================="

echo ""
echo "--- Restart service via API ---"
resp=$(api POST "/api/service/stop" "{}")
echo "$resp"
sleep 1
resp=$(api POST "/api/service/start" "{}")
echo "$resp"
if check_code "$resp"; then
    pass "restart sequence works"
else
    fail "restart sequence failed"
fi

# ============================================
# HTML STRUCTURE TESTS
# ============================================
echo ""
echo "=========================================="
echo "HTML STRUCTURE TESTS"
echo "=========================================="

echo ""
echo "--- Checking HTML page loads ---"
html=$(curl -s "$BASE_URL/")
if echo "$html" | grep -q "Control Tower"; then
    pass "HTML page loads"
else
    fail "HTML page failed to load"
fi

echo ""
echo "--- Checking all 6 views exist in HTML ---"
views=("view-dashboard" "view-proxies" "view-profiles" "view-rules" "view-connections" "view-settings")
for v in "${views[@]}"; do
    if echo "$html" | grep -q "$v"; then
        pass "view exists: $v"
    else
        fail "view missing: $v"
    fi
done

echo ""
echo "--- Checking key JS functions exist ---"
funcs=("loadDashboard" "loadProxies" "loadProfiles" "loadRules" "loadConnections" "startService" "stopService" "setMode" "selectProxy" "addProfile" "activateProfile" "deleteProfile" "addRule" "closeConnection")
for f in "${funcs[@]}"; do
    if echo "$html" | grep -q "function $f"; then
        pass "JS function exists: $f"
    else
        fail "JS function missing: $f"
    fi
done

echo ""
echo "=========================================="
echo "KNOWN ISSUES FOUND"
echo "=========================================="
echo "1. Rules page: addRule() shows 'not implemented' toast, no actual persistence"
echo "2. Rules page: deleteRule() shows 'not implemented' toast"
echo "3. Rules page: Rules are read-only from config, no write API exists"
echo "4. Connections page: closeConnection requires active connection ID"
echo "5. Profile add: Network timeout issue (profile add is async, UI doesn't block)"
echo "6. Mode setting: After service stop, mode buttons may be out of sync"
echo ""
echo "=========================================="
echo "TEST COMPLETE"
echo "=========================================="
