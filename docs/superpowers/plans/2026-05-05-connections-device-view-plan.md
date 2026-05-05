# Connections 页面设备视图实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 在 Connections 页面新增两个 Tab（活跃设备 + 最近设备），按 sourceIP 聚合展示设备和实时流量

**架构：** 前端聚合 Mihomo 的 `/api/connections` 和 `/api/connections/history` 数据，无需后端改动。活跃设备按 sourceIP 聚合并计算实时速度；最近设备复用现有 history API 结果按 IP 聚合。

**技术栈：** Vanilla JavaScript、现有 HTML/CSS、Actix-web API

---

## 文件变更概览

| 文件 | 职责 |
|------|------|
| `crates/control-tower-service/ui/index.html` | 添加设备 Tab UI、聚合 JS、设备卡片渲染逻辑 |

---

## 任务 1：添加设备 Tab 按钮

**文件：** 修改 `crates/control-tower-service/ui/index.html:697-700`

- [ ] **步骤 1：在 Tab 栏添加两个新按钮**

在现有的两个 Tab 按钮后添加：

```html
<button class="tab-btn" id="tab-conn-devices-btn" onclick="switchConnTab('devices')">Active Devices</button>
<button class="tab-btn" id="tab-conn-recent-btn" onclick="switchConnTab('recent')">Recent Devices</button>
```

- [ ] **步骤 2：验证 HTML 结构**

运行：`grep -n "tab-conn-" crates/control-tower-service/ui/index.html | head -20`
预期：看到 active/history/devices/recent 四个 tab-btn

- [ ] **步骤 3：Commit**

```bash
git add crates/control-tower-service/ui/index.html
git commit -m "feat(conn): add devices and recent tabs to connections page"
```

---

## 任务 2：添加设备 Tab 内容区

**文件：** 修改 `crates/control-tower-service/ui/index.html`，在 History Tab 的 `</div>`（约775行）前添加

- [ ] **步骤 1：在 History Tab 结束后添加两个新 Tab 的容器**

```html
<!-- Active Devices Tab -->
<div id="conn-devices-tab" style="display:none">
  <div class="stat-grid" style="margin-bottom:20px">
    <div class="stat-card">
      <div class="stat-icon blue"><svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><rect x="5" y="2" width="14" height="20" rx="2"/><line x1="12" y1="18" x2="12" y2="18"/></svg></div>
      <div class="stat-info"><div class="stat-value" id="dev-total">0</div><div class="stat-label">Active Devices</div></div>
    </div>
    <div class="stat-card">
      <div class="stat-icon green"><svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 10l7-7m0 0l7 7m-7-7v18"/></svg></div>
      <div class="stat-info"><div class="stat-value mono text-sm" id="dev-upload">0 B/s</div><div class="stat-label">Upload Rate</div></div>
    </div>
    <div class="stat-card">
      <div class="stat-icon yellow"><svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 14l-7 7m0 0l-7-7m7 7V3"/></svg></div>
      <div class="stat-info"><div class="stat-value mono text-sm" id="dev-download">0 B/s</div><div class="stat-label">Download Rate</div></div>
    </div>
  </div>
  <div id="conn-devices-loading" class="loading"><div class="spinner"></div>Loading devices...</div>
  <div id="conn-devices-list"></div>
  <div id="conn-devices-empty" class="empty-state" style="display:none">
    <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><rect x="5" y="2" width="14" height="20" rx="2"/></svg>
    <p>No active devices</p>
    <div class="empty-action text-sm text-muted">Devices connected to the proxy will appear here</div>
  </div>
</div>

<!-- Recent Devices Tab -->
<div id="conn-recent-tab" style="display:none">
  <div style="display:flex;justify-content:flex-end;margin-bottom:12px">
    <button class="btn ghost sm" id="clearRecentHistoryBtn" onclick="clearConnectionHistory()" style="color:#f87171">
      <svg width="13" height="13" fill="none" stroke="currentColor" viewBox="0 0 24 24"><polyline points="3,6 5,6 21,6"/><path d="M19,6l-1,14a2,2,0,0,1-2,2H8a2,2,0,0,1-2-2L5,6"/></svg>
      Clear History
    </button>
  </div>
  <div id="conn-recent-loading" class="loading"><div class="spinner"></div>Loading recent devices...</div>
  <div id="conn-recent-list"></div>
  <div id="conn-recent-empty" class="empty-state" style="display:none">
    <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><rect x="5" y="2" width="14" height="20" rx="2"/></svg>
    <p>No recent devices</p>
    <div class="empty-action text-sm text-muted">Recently disconnected devices will appear here</div>
  </div>
</div>
```

- [ ] **步骤 2：验证 HTML**

运行：`grep -n "conn-devices-tab\|conn-recent-tab" crates/control-tower-service/ui/index.html`
预期：两行，id 匹配

- [ ] **步骤 3：Commit**

```bash
git add crates/control-tower-service/ui/index.html
git commit -m "feat(conn): add devices and recent tab content containers"
```

---

## 任务 3：添加 Tab 切换逻辑和设备聚合数据结构

**文件：** 修改 `crates/control-tower-service/ui/index.html`，在 `switchConnTab` 定义附近

- [ ] **步骤 1：在现有全局变量区域添加设备相关变量**

约在第 2450 行 `async function loadConnections()` 前添加：

```javascript
let currentConnTab = 'active';
let connRefreshTimer = null;
let connPaused = false;
let connSortCol = null, connSortDir = 'desc';
let connections = []; // raw connections for active tab

// Device tracking for speed calculation
let prevDeviceStats = {}; // { sourceIP: { upload, download, ts } }
let deviceAggregationTimer = null;
```

- [ ] **步骤 2：更新 `switchConnTab` 函数**

约在 2310 行找到 `function switchConnTab(tab)`，替换为：

```javascript
function switchConnTab(tab) {
  currentConnTab = tab;
  // Stop device aggregation timer when switching away
  if (deviceAggregationTimer) { clearInterval(deviceAggregationTimer); deviceAggregationTimer = null; }

  // Update tab button states
  document.getElementById('tab-conn-active-btn').classList.toggle('active', tab === 'active');
  document.getElementById('tab-conn-history-btn').classList.toggle('active', tab === 'history');
  document.getElementById('tab-conn-devices-btn').classList.toggle('active', tab === 'devices');
  document.getElementById('tab-conn-recent-btn').classList.toggle('active', tab === 'recent');

  // Show/hide tab content
  document.getElementById('conn-active-tab').style.display = tab === 'active' ? 'block' : 'none';
  document.getElementById('conn-history-tab').style.display = tab === 'history' ? 'block' : 'none';
  document.getElementById('conn-devices-tab').style.display = tab === 'devices' ? 'block' : 'none';
  document.getElementById('conn-recent-tab').style.display = tab === 'recent' ? 'block' : 'none';

  // Start loading data for the selected tab
  if (tab === 'active') { loadConnections(); onConnRefreshChange(); }
  else if (tab === 'devices') { loadActiveDevices(); startDevicePolling(); }
  else if (tab === 'recent') { loadRecentDevices(); }
  else if (tab === 'history') { loadConnectionHistory(); }
}
```

- [ ] **步骤 3：验证 `switchConnTab` 更新**

运行：`grep -n "currentConnTab\|deviceAggregationTimer" crates/control-tower-service/ui/index.html`
预期：能看到新变量使用

- [ ] **步骤 4：Commit**

```bash
git add crates/control-tower-service/ui/index.html
git commit -m "feat(conn): add switchConnTab for 4 tabs and device polling state"
```

---

## 任务 4：实现 `loadActiveDevices()` 函数

**文件：** 修改 `crates/control-tower-service/ui/index.html`，在 `loadConnections()` 函数后添加

- [ ] **步骤 1：添加 `loadActiveDevices()` 函数**

在约 2530 行 `function goConnPage(p)` 前添加：

```javascript
async function loadActiveDevices() {
  const loadingEl = document.getElementById('conn-devices-loading');
  const listEl = document.getElementById('conn-devices-list');
  const emptyEl = document.getElementById('conn-devices-empty');
  const devTotalEl = document.getElementById('dev-total');
  const devUploadEl = document.getElementById('dev-upload');
  const devDownloadEl = document.getElementById('dev-download');

  try {
    const data = await api('GET', '/api/connections');
    const conns = data.connections || [];

    // Aggregate by sourceIP
    const deviceMap = {};
    conns.forEach(conn => {
      const meta = conn.metadata || {};
      const ip = meta.sourceIP || 'unknown';
      if (!deviceMap[ip]) {
        deviceMap[ip] = {
          ip,
          connections: [],
          upload: 0,
          download: 0,
          uploadRate: 0,
          downloadRate: 0
        };
      }
      deviceMap[ip].connections.push(conn);
      deviceMap[ip].upload += conn.upload || 0;
      deviceMap[ip].download += conn.download || 0;
    });

    // Calculate speed (rate) from previous snapshot
    const now = Date.now();
    Object.values(deviceMap).forEach(dev => {
      const prev = prevDeviceStats[dev.ip];
      if (prev && prev.ts) {
        const dt = (now - prev.ts) / 1000; // seconds
        if (dt > 0) {
          dev.uploadRate = Math.max(0, (dev.upload - prev.upload) / dt);
          dev.downloadRate = Math.max(0, (dev.download - prev.download) / dt);
        }
      }
      prevDeviceStats[dev.ip] = { upload: dev.upload, download: dev.download, ts: now };
    });

    // Update stat cards
    const deviceCount = Object.keys(deviceMap).length;
    let totalUpload = 0, totalDownload = 0;
    Object.values(deviceMap).forEach(d => { totalUpload += d.uploadRate; totalDownload += d.downloadRate; });
    devTotalEl.textContent = deviceCount;
    devUploadEl.textContent = formatBytes(totalUpload) + '/s';
    devDownloadEl.textContent = formatBytes(totalDownload) + '/s';

    // Render device cards
    if (deviceCount === 0) {
      loadingEl.style.display = 'none';
      listEl.innerHTML = '';
      emptyEl.style.display = 'block';
      return;
    }

    loadingEl.style.display = 'none';
    emptyEl.style.display = 'none';

    // Sort by total bandwidth (download + upload)
    const sorted = Object.values(deviceMap).sort((a, b) => (b.uploadRate + b.downloadRate) - (a.uploadRate + a.downloadRate));

    let html = '';
    sorted.forEach((dev, idx) => {
      const connCount = dev.connections.length;
      html += `<div class="device-card" style="background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius);margin-bottom:12px;overflow:hidden">
        <div class="device-card-header" onclick="toggleDeviceCard(${idx})" style="display:flex;align-items:center;padding:14px 16px;cursor:pointer;user-select:none">
          <div style="flex:1">
            <div style="font-weight:600;font-size:14px;display:flex;align-items:center;gap:8px">
              <span style="color:var(--text2)">📱</span>
              <span class="mono">${dev.ip}</span>
            </div>
            <div style="font-size:12px;color:var(--text3);margin-top:4px">
              <span style="background:var(--accent-light);color:var(--accent);padding:2px 8px;border-radius:10px;font-size:11px">${connCount} connection${connCount !== 1 ? 's' : ''}</span>
              <span style="margin-left:12px" class="mono">↑ ${formatBytes(dev.uploadRate)}/s</span>
              <span style="margin-left:8px" class="mono">↓ ${formatBytes(dev.downloadRate)}/s</span>
            </div>
          </div>
          <span class="device-expand-icon" id="dev-expand-${idx}" style="color:var(--text3);transition:transform 150ms">▼</span>
        </div>
        <div class="device-card-body" id="dev-body-${idx}" style="display:none;border-top:1px solid var(--border);padding:12px 16px;background:var(--bg)">
          ${dev.connections.map(c => {
            const meta = c.metadata || {};
            const dst = meta.host || ((meta.destinationIP || '') + (meta.destinationPort ? ':' + meta.destinationPort : ''));
            const chain = (c.chains || []).slice(1).join(' → ') || 'DIRECT';
            return `<div style="display:flex;align-items:center;padding:6px 0;border-bottom:1px solid var(--border);font-size:12px">
              <div style="flex:1;min-width:0">
                <div class="mono" style="color:var(--text);white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${dst || '-'}</div>
                <div style="color:var(--text3);font-size:11px">${chain}</div>
              </div>
              <div class="mono" style="color:var(--success);font-size:11px;margin-left:8px">↑ ${formatBytes(c.upload || 0)}</div>
              <div class="mono" style="color:var(--warning);font-size:11px;margin-left:8px">↓ ${formatBytes(c.download || 0)}</div>
            </div>`;
          }).join('')}
        </div>
      </div>`;
    });

    listEl.innerHTML = html;
  } catch (e) {
    console.error('Failed to load active devices:', e);
    loadingEl.style.display = 'none';
    listEl.innerHTML = '<div style="color:var(--danger);padding:16px">Failed to load devices</div>';
  }
}

function toggleDeviceCard(idx) {
  const body = document.getElementById('dev-body-' + idx);
  const icon = document.getElementById('dev-expand-' + idx);
  const isHidden = body.style.display === 'none';
  body.style.display = isHidden ? 'block' : 'none';
  icon.style.transform = isHidden ? 'rotate(180deg)' : 'rotate(0deg)';
}
```

- [ ] **步骤 2：添加 `startDevicePolling()` 函数**

在 `loadActiveDevices()` 后添加：

```javascript
function startDevicePolling() {
  if (deviceAggregationTimer) { clearInterval(deviceAggregationTimer); }
  const intervalEl = document.getElementById('conn-refresh-interval');
  const interval = intervalEl ? parseInt(intervalEl.value) : 5000;
  if (interval > 0) {
    deviceAggregationTimer = setInterval(loadActiveDevices, interval);
  }
}
```

- [ ] **步骤 3：验证函数已添加**

运行：`grep -n "loadActiveDevices\|startDevicePolling\|toggleDeviceCard" crates/control-tower-service/ui/index.html`
预期：三个函数都有定义

- [ ] **步骤 4：Commit**

```bash
git add crates/control-tower-service/ui/index.html
git commit -m "feat(conn): implement loadActiveDevices with sourceIP aggregation and speed calculation"
```

---

## 任务 5：实现 `loadRecentDevices()` 函数

**文件：** 修改 `crates/control-tower-service/ui/index.html`，在 `loadActiveDevices()` 后添加

- [ ] **步骤 1：添加 `loadRecentDevices()` 函数**

```javascript
async function loadRecentDevices() {
  const loadingEl = document.getElementById('conn-recent-loading');
  const listEl = document.getElementById('conn-recent-list');
  const emptyEl = document.getElementById('conn-recent-empty');

  try {
    const data = await api('GET', '/api/connections/history');
    const history = data || [];

    if (history.length === 0) {
      loadingEl.style.display = 'none';
      listEl.innerHTML = '';
      emptyEl.style.display = 'block';
      return;
    }

    loadingEl.style.display = 'none';
    emptyEl.style.display = 'none';

    // Aggregate by source_ip
    const deviceMap = {};
    history.forEach(conn => {
      const ip = conn.source_ip || 'unknown';
      if (!deviceMap[ip]) {
        deviceMap[ip] = {
          ip,
          totalUpload: 0,
          totalDownload: 0,
          lastActive: null,
          lastDestination: null,
          lastChain: null,
          connections: []
        };
      }
      deviceMap[ip].totalUpload += conn.upload || 0;
      deviceMap[ip].totalDownload += conn.download || 0;
      deviceMap[ip].connections.push(conn);

      // Track most recent connection
      if (!deviceMap[ip].lastActive || (conn.closed_at && conn.closed_at > deviceMap[ip].lastActive)) {
        deviceMap[ip].lastActive = conn.closed_at;
        deviceMap[ip].lastDestination = conn.destination;
        deviceMap[ip].lastChain = (conn.chains || []).join(' → ');
      }
    });

    // Sort by lastActive (most recent first)
    const sorted = Object.values(deviceMap).sort((a, b) => {
      if (!a.lastActive) return 1;
      if (!b.lastActive) return -1;
      return new Date(b.lastActive) - new Date(a.lastActive);
    });

    let html = '';
    sorted.forEach((dev, idx) => {
      const timeAgo = dev.lastActive ? formatTimeAgo(dev.lastActive) : 'unknown';
      html += `<div class="device-card" style="background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius);margin-bottom:12px;overflow:hidden">
        <div class="device-card-header" onclick="toggleRecentDeviceCard(${idx})" style="display:flex;align-items:center;padding:14px 16px;cursor:pointer;user-select:none">
          <div style="flex:1">
            <div style="font-weight:600;font-size:14px;display:flex;align-items:center;gap:8px">
              <span style="color:var(--text2)">📱</span>
              <span class="mono">${dev.ip}</span>
              <span style="background:var(--bg4);color:var(--text3);padding:2px 8px;border-radius:10px;font-size:11px">disconnected</span>
            </div>
            <div style="font-size:12px;color:var(--text3);margin-top:4px">
              <span>Last active: ${timeAgo}</span>
            </div>
          </div>
          <span class="device-expand-icon" id="recent-expand-${idx}" style="color:var(--text3);transition:transform 150ms">▼</span>
        </div>
        <div class="device-card-body" id="recent-body-${idx}" style="display:none;border-top:1px solid var(--border);padding:12px 16px;background:var(--bg)">
          <div style="margin-bottom:10px;font-size:12px">
            <div style="color:var(--text3);margin-bottom:4px">Cumulative Traffic</div>
            <div><span class="mono" style="color:var(--success)">↑ ${formatBytes(dev.totalUpload)}</span> <span class="mono" style="color:var(--warning);margin-left:16px">↓ ${formatBytes(dev.totalDownload)}</span></div>
          </div>
          ${dev.lastDestination ? `<div style="font-size:12px;border-top:1px solid var(--border);padding-top:10px">
            <div style="color:var(--text3);margin-bottom:4px">Last Connection</div>
            <div class="mono" style="color:var(--text)">${dev.lastDestination}</div>
            <div style="color:var(--text3);font-size:11px">${dev.lastChain}</div>
          </div>` : ''}
          <div style="border-top:1px solid var(--border);padding-top:10px;margin-top:10px;font-size:11px;color:var(--text3)">
            ${dev.connections.length} total connection${dev.connections.length !== 1 ? 's' : ''} recorded
          </div>
        </div>
      </div>`;
    });

    listEl.innerHTML = html;
  } catch (e) {
    console.error('Failed to load recent devices:', e);
    loadingEl.style.display = 'none';
    listEl.innerHTML = '<div style="color:var(--danger);padding:16px">Failed to load recent devices</div>';
  }
}

function toggleRecentDeviceCard(idx) {
  const body = document.getElementById('recent-body-' + idx);
  const icon = document.getElementById('recent-expand-' + idx);
  const isHidden = body.style.display === 'none';
  body.style.display = isHidden ? 'block' : 'none';
  icon.style.transform = isHidden ? 'rotate(180deg)' : 'rotate(0deg)';
}
```

- [ ] **步骤 2：添加 `formatTimeAgo()` 辅助函数**

在文件顶部 `formatBytes` 函数附近添加：

```javascript
function formatTimeAgo(isoString) {
  if (!isoString) return 'unknown';
  const diff = Date.now() - new Date(isoString).getTime();
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return seconds + 's ago';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes + 'm ago';
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return hours + 'h ago';
  const days = Math.floor(hours / 24);
  return days + 'd ago';
}
```

- [ ] **步骤 3：验证函数已添加**

运行：`grep -n "loadRecentDevices\|formatTimeAgo\|toggleRecentDeviceCard" crates/control-tower-service/ui/index.html`
预期：能看到函数定义

- [ ] **步骤 4：Commit**

```bash
git add crates/control-tower-service/ui/index.html
git commit -m "feat(conn): implement loadRecentDevices with source_ip aggregation and time display"
```

---

## 任务 6：更新 Tab 切换时的定时器清理

**文件：** 修改 `crates/control-tower-service/ui/index.html:1425-1435` `switchView` 函数

- [ ] **步骤 1：在 switchView 的定时器清理处添加设备相关的定时器清理**

约在 1428 行，找到：
```javascript
if (dashRefreshTimer) { clearInterval(dashRefreshTimer); dashRefreshTimer = null; }
```

添加：
```javascript
if (connRefreshTimer) { clearInterval(connRefreshTimer); connRefreshTimer = null; }
if (deviceAggregationTimer) { clearInterval(deviceAggregationTimer); deviceAggregationTimer = null; }
```

- [ ] **步骤 2：验证**

运行：`grep -n "deviceAggregationTimer" crates/control-tower-service/ui/index.html`
预期：至少 3 处：定义、setInterval、clearInterval in switchView

- [ ] **步骤 3：Commit**

```bash
git add crates/control-tower-service/ui/index.html
git commit -m "fix(conn): clear device polling timer when switching views"
```

---

## 任务 7：验证和测试

**文件：** 无

- [ ] **步骤 1：构建验证**

运行：`cargo build 2>&1 | grep warning`
预期：无 warning

- [ ] **步骤 2：检查 HTML 无语法错误**

运行：`grep -c "function loadActiveDevices\|function loadRecentDevices" crates/control-tower-service/ui/index.html`
预期：2

- [ ] **步骤 3：部署并验证**

运行：`sh build.sh && systemctl stop ctsvc && cp target/release/ctsvc /opt/ctsvc/ctsvc && systemctl start ctsvc`
预期：部署成功

- [ ] **步骤 4：浏览器测试场景**

1. 打开 Connections 页面，确认有 4 个 Tab
2. 切换到 "Active Devices" Tab，确认能看到设备卡片（如果有连接）
3. 切换到 "Recent Devices" Tab，确认能看到历史设备（如果有关闭的连接）
4. 切换回 "Active" Tab，确认定时器正常工作
5. 打开 DevTools Console，确认无 JS 错误

- [ ] **步骤 5：Commit**

```bash
git add -A && git commit -m "feat(conn): complete devices and recent tabs for connections page"
```

---

## 验收标准检查

| 标准 | 验证方式 |
|------|----------|
| 活跃设备 Tab 显示所有当前连接的设备 IP | 浏览器打开 Tab，看设备卡片数量 |
| 流量聚合正确 | 多个连接同 IP 时，聚合速度/流量显示正确 |
| 最近设备 Tab 显示历史连接按 IP 聚合 | 关闭几个连接，切换到 Recent Tab 验证 |
| 切换 Tab 时正确清空定时器 | 切换 Tab 后 DevTools 看网络请求是否停止 |
| 无设备时显示空状态 | 没有任何连接时切换到 Devices Tab 看空状态 |