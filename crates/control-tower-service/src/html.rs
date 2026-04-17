//! Embedded Web UI HTML template

pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Control Tower</title>
<style>
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap');

*{margin:0;padding:0;box-sizing:border-box}
:root{
--bg:#f8fafc;--bg2:#fff;--bg3:#f1f5f9;--border:#e2e8f0;
--text:#0f172a;--text2:#475569;--text3:#94a3b8;
--accent:#3b82f6;--accent-hover:#2563eb;
--success:#22c55e;--warning:#eab308;--danger:#ef4444;
--sidebar-w:200px;--header-h:56px;
}
@media(prefers-color-scheme:dark){
:root{
--bg:#0f172a;--bg2:#1e293b;--bg3:#334155;--border:#475569;
--text:#f1f5f9;--text2:#cbd5e1;--text3:#64748b;
}}
body{font-family:'Inter',system-ui,sans-serif;background:var(--bg);color:var(--text);transition:background 150ms ease,color 150ms ease}
.mono{font-family:'JetBrains Mono',monospace}

/* Layout */
.app{display:flex;flex-direction:column;height:100vh}
.header{position:fixed;top:0;left:0;right:0;height:var(--header-h);background:var(--bg2);border-bottom:1px solid var(--border);display:flex;align-items:center;padding:0 20px;z-index:100;gap:12px}
.header .logo{width:32px;height:32px;background:var(--accent);border-radius:8px;display:flex;align-items:center;justify-content:center}
.header .logo svg{width:20px;height:20px;fill:#fff}
.header h1{font-size:16px;font-weight:600}
.status-dot{width:8px;height:8px;border-radius:50%;background:var(--success);margin-left:auto}
.status-dot.stopped{background:var(--danger)}
.main{display:flex;flex:1;padding-top:var(--header-h)}
.sidebar{position:fixed;left:0;top:var(--header-h);bottom:0;width:var(--sidebar-w);background:var(--bg2);border-right:1px solid var(--border);padding:16px 8px;display:flex;flex-direction:column;gap:4px}
.nav-item{display:flex;align-items:center;gap:12px;padding:10px 12px;border-radius:8px;color:var(--text2);cursor:pointer;transition:all 150ms ease;font-size:14px;font-weight:500}
.nav-item:hover{background:var(--bg3);color:var(--text)}
.nav-item.active{background:var(--accent);color:#fff}
.nav-item svg{width:20px;height:20px;flex-shrink:0}
.content{margin-left:var(--sidebar-w);flex:1;padding:32px;max-width:1200px;width:100%;overflow-y:auto}
@media(max-width:768px){
.sidebar{top:auto;bottom:0;left:0;right:0;width:100%;height:64px;flex-direction:row;padding:8px;justify-content:space-around;border-right:none;border-top:1px solid var(--border)}
.nav-item{flex-direction:column;gap:4px;font-size:10px;padding:8px}
.nav-item span{display:none}
.content{margin-left:0;margin-bottom:64px;padding:20px}
}

/* Cards */
.card{background:var(--bg2);border:1px solid var(--border);border-radius:12px;padding:24px;margin-bottom:24px}
.card-title{font-size:14px;font-weight:600;color:var(--text2);margin-bottom:16px;text-transform:uppercase;letter-spacing:.5px}
.card-value{font-size:32px;font-weight:600;color:var(--text);margin-bottom:4px}
.card-sub{font-size:13px;color:var(--text3)}

/* Buttons */
.btn{display:inline-flex;align-items:center;justify-content:center;gap:8px;padding:10px 20px;border-radius:8px;font-size:14px;font-weight:500;cursor:pointer;transition:all 150ms ease;border:none;background:var(--accent);color:#fff}
.btn:hover{background:var(--accent-hover)}
.btn:active{transform:scale(0.98)}
.btn.secondary{background:var(--bg3);color:var(--text)}
.btn.danger{background:var(--danger);color:#fff}
.btn.success{background:var(--success);color:#fff}
.btn.sm{padding:6px 12px;font-size:12px}
.btn:disabled{opacity:.5;cursor:not-allowed}
.btn-group{display:flex;gap:8px;flex-wrap:wrap}

/* Forms */
.form-group{margin-bottom:16px}
.form-label{display:block;font-size:13px;font-weight:500;color:var(--text2);margin-bottom:6px}
.form-input{width:100%;padding:10px 14px;border:1px solid var(--border);border-radius:8px;background:var(--bg);color:var(--text);font-size:14px;transition:border-color 150ms ease}
.form-input:focus{outline:none;border-color:var(--accent)}
.form-select{width:100%;padding:10px 14px;border:1px solid var(--border);border-radius:8px;background:var(--bg);color:var(--text);font-size:14px;cursor:pointer}

/* Tables */
.table{width:100%;border-collapse:collapse}
.table th{text-align:left;padding:12px 16px;font-size:12px;font-weight:600;color:var(--text3);text-transform:uppercase;letter-spacing:.5px;border-bottom:1px solid var(--border)}
.table td{padding:12px 16px;font-size:14px;border-bottom:1px solid var(--border);vertical-align:middle}
.table tr:hover td{background:var(--bg3)}
.table .truncate{max-width:200px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}

/* Status indicators */
.badge{display:inline-block;padding:4px 10px;border-radius:20px;font-size:12px;font-weight:500}
.badge.success{background:rgba(34,197,94,.15);color:var(--success)}
.badge.warning{background:rgba(234,179,8,.15);color:var(--warning)}
.badge.danger{background:rgba(239,68,68,.15);color:var(--danger)}
.badge.info{background:rgba(59,130,246,.15);color:var(--accent)}

/* Latency colors */
.latency-good{color:var(--success)}
.latency-medium{color:var(--warning)}
.latency-bad{color:var(--danger)}
.latency-timeout{color:var(--text3)}

/* Active profile border */
.profile-active{border-left:3px solid var(--accent);padding-left:12px}

/* Toast */
.toast{position:fixed;bottom:24px;right:24px;padding:12px 20px;background:var(--bg2);border:1px solid var(--border);border-radius:8px;box-shadow:0 4px 12px rgba(0,0,0,.15);z-index:1000;display:none;animation:slideIn 150ms ease}
.toast.show{display:block}
.toast.success{border-left:3px solid var(--success)}
.toast.error{border-left:3px solid var(--danger)}
@keyframes slideIn{from{transform:translateX(100%);opacity:0}to{transform:translateX(0);opacity:1}}

/* Modal */
.modal{position:fixed;inset:0;background:rgba(0,0,0,.5);display:none;align-items:center;justify-content:center;z-index:200}
.modal.show{display:flex}
.modal-content{background:var(--bg2);border-radius:12px;padding:24px;max-width:400px;width:90%}

/* Loading */
.loading{text-align:center;padding:40px;color:var(--text3)}
.spinner{display:inline-block;width:24px;height:24px;border:2px solid var(--border);border-top-color:var(--accent);border-radius:50%;animation:spin 1s linear infinite;margin-right:12px}
@keyframes spin{to{transform:rotate(360deg)}

/* Empty state */
.empty-state{text-align:center;padding:60px 20px;color:var(--text3)}
.empty-state svg{width:48px;height:48px;margin-bottom:16px;opacity:.5}

/* Stats bar */
.stats-bar{display:flex;gap:24px;padding:16px 0;border-bottom:1px solid var(--border);margin-bottom:16px;flex-wrap:wrap}
.stat-item{display:flex;align-items:center;gap:8px}
.stat-value{font-weight:600;font-size:16px}
.stat-label{font-size:13px;color:var(--text3)}

/* Pagination */
.pagination{display:flex;align-items:center;justify-content:center;gap:8px;margin-top:20px}
.page-btn{padding:8px 14px;border:1px solid var(--border);border-radius:6px;background:var(--bg2);color:var(--text);cursor:pointer;font-size:13px}
.page-btn:hover{background:var(--bg3)}
.page-btn.active{background:var(--accent);color:#fff;border-color:var(--accent)}
.page-info{font-size:13px;color:var(--text3)}

/* Search */
.search-bar{max-width:300px;margin-left:auto}
.search-input{width:100%;padding:8px 14px;border:1px solid var(--border);border-radius:8px;background:var(--bg);color:var(--text);font-size:13px}
.search-input:focus{outline:none;border-color:var(--accent)}

/* View containers */
.view{display:none}
.view.active{display:block}

/* Flex utilities */
.flex{display:flex}
.flex-col{flex-direction:column}
.items-center{align-items:center}
.justify-between{justify-content:space-between}
.gap-8{gap:8px}
.gap-16{gap:16px}
.mt-16{margin-top:16px}
.mb-16{margin-bottom:16px}
.w-full{width:100%}
.text-center{text-align:center}
</style>
</head>
<body>
<div class="app">
  <header class="header">
    <div class="logo">
      <svg viewBox="0 0 24 24"><path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg>
    </div>
    <h1>Control Tower</h1>
    <div class="status-dot" id="headerStatus" title="Service Status"></div>
  </header>

  <main class="main">
    <nav class="sidebar">
      <div class="nav-item active" data-view="dashboard">
        <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6"/></svg>
        <span>Dashboard</span>
      </div>
      <div class="nav-item" data-view="proxies">
        <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2"/></svg>
        <span>Proxies</span>
      </div>
      <div class="nav-item" data-view="profiles">
        <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"/></svg>
        <span>Profiles</span>
      </div>
      <div class="nav-item" data-view="rules">
        <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-3 7h3m-3 4h3m-6-4h.01M9 16h.01"/></svg>
        <span>Rules</span>
      </div>
      <div class="nav-item" data-view="connections">
        <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"/></svg>
        <span>Connections</span>
      </div>
      <div class="nav-item" data-view="settings">
        <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"/><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
        <span>Settings</span>
      </div>
    </nav>

    <div class="content">
      <!-- Dashboard View -->
      <div class="view active" id="view-dashboard">
        <div class="card">
          <div class="card-title">Service Status</div>
          <div class="flex items-center gap-16">
            <div>
              <div class="card-value" id="dash-status">Loading...</div>
              <div class="card-sub" id="dash-uptime"></div>
            </div>
            <div class="btn-group" style="margin-left:auto">
              <button class="btn success" id="btn-start" onclick="startService()">Start</button>
              <button class="btn danger" id="btn-stop" onclick="stopService()">Stop</button>
            </div>
          </div>
        </div>

        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(250px,1fr));gap:24px">
          <div class="card">
            <div class="card-title">Current Mode</div>
            <div class="card-value" id="dash-mode">Loading...</div>
            <div class="btn-group mt-16">
              <button class="btn secondary sm" onclick="setMode('rule')">Rule</button>
              <button class="btn secondary sm" onclick="setMode('global')">Global</button>
              <button class="btn secondary sm" onclick="setMode('direct')">Direct</button>
            </div>
          </div>

          <div class="card">
            <div class="card-title">Current Proxy</div>
            <div class="card-value mono" id="dash-proxy" style="font-size:18px">Loading...</div>
            <div class="card-sub mt-16">Click Proxies to select</div>
          </div>

          <div class="card">
            <div class="card-title">Quick Stats</div>
            <div class="flex flex-col gap-8">
              <div><span class="text2">Online Nodes:</span> <span id="dash-nodes" class="mono">-</span></div>
              <div><span class="text2">Connections:</span> <span id="dash-connections" class="mono">-</span></div>
            </div>
          </div>
        </div>
      </div>

      <!-- Proxies View -->
      <div class="view" id="view-proxies">
        <div class="flex justify-between items-center mb-16">
          <h2>Proxy Nodes</h2>
          <div class="search-bar">
            <input type="text" class="search-input" id="proxy-search" placeholder="Search proxies..." oninput="filterProxies()">
          </div>
        </div>
        <div class="card" style="padding:0;overflow:hidden">
          <div id="proxy-loading" class="loading"><div class="spinner"></div>Loading proxies...</div>
          <table class="table" id="proxy-table" style="display:none">
            <thead><tr><th>Name</th><th>Type</th><th>Latency</th><th>Status</th></tr></thead>
            <tbody id="proxy-list"></tbody>
          </table>
          <div id="proxy-empty" class="empty-state" style="display:none">
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2"/></svg>
            <p>No proxies found</p>
          </div>
        </div>
      </div>

      <!-- Profiles View -->
      <div class="view" id="view-profiles">
        <h2 class="mb-16">Profiles</h2>
        <div class="card">
          <div class="card-title">Add Profile</div>
          <form onsubmit="addProfile(event)" class="flex gap-8" style="align-items:flex-end">
            <div class="form-group" style="flex:1;margin-bottom:0">
              <label class="form-label">URL</label>
              <input type="url" class="form-input" id="profile-url" placeholder="https://example.com/profile.yaml" required>
            </div>
            <div class="form-group" style="flex:1;margin-bottom:0">
              <label class="form-label">Name (optional)</label>
              <input type="text" class="form-input" id="profile-name" placeholder="My Profile">
            </div>
            <button type="submit" class="btn">Add</button>
          </form>
        </div>
        <div class="card" style="padding:0;overflow:hidden">
          <div id="profiles-loading" class="loading"><div class="spinner"></div>Loading profiles...</div>
          <table class="table" id="profiles-table" style="display:none">
            <thead><tr><th>Name</th><th>URL</th><th>Status</th><th>Operations</th></tr></thead>
            <tbody id="profiles-list"></tbody>
          </table>
          <div id="profiles-empty" class="empty-state" style="display:none">
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"/></svg>
            <p>No profiles found</p>
          </div>
        </div>
      </div>

      <!-- Rules View -->
      <div class="view" id="view-rules">
        <h2 class="mb-16">Rules</h2>
        <div class="card">
          <div class="card-title">Add Rule</div>
          <form onsubmit="addRule(event)" class="flex gap-8" style="align-items:flex-end">
            <div class="form-group" style="margin-bottom:0">
              <label class="form-label">Type</label>
              <select class="form-select" id="rule-type" style="width:auto;min-width:120px">
                <option value="DOMAIN">DOMAIN</option>
                <option value="DOMAIN-SUFFIX">DOMAIN-SUFFIX</option>
                <option value="DOMAIN-KEYWORD">DOMAIN-KEYWORD</option>
                <option value="GEOIP">GEOIP</option>
                <option value="IP-CIDR">IP-CIDR</option>
                <option value="IP-CIDR6">IP-CIDR6</option>
                <option value="PROCESS-NAME">PROCESS-NAME</option>
                <option value="RULE-SET">RULE-SET</option>
              </select>
            </div>
            <div class="form-group" style="flex:1;margin-bottom:0">
              <label class="form-label">Value</label>
              <input type="text" class="form-input" id="rule-value" placeholder="example.com" required>
            </div>
            <div class="form-group" style="flex:1;margin-bottom:0">
              <label class="form-label">Proxy</label>
              <input type="text" class="form-input" id="rule-proxy" placeholder="Proxy name or DIRECT" required>
            </div>
            <button type="submit" class="btn">Add</button>
          </form>
          <p style="margin-top:12px;font-size:13px;color:var(--text3)">Note: Rules API may not be fully implemented. Rules changes may not persist.</p>
        </div>
        <div class="card" style="padding:0;overflow:hidden">
          <div id="rules-loading" class="loading"><div class="spinner"></div>Loading rules...</div>
          <table class="table" id="rules-table" style="display:none">
            <thead><tr><th style="width:60px">#</th><th>Type</th><th>Value</th><th>Proxy</th><th style="width:80px">Op</th></tr></thead>
            <tbody id="rules-list"></tbody>
          </table>
          <div id="rules-empty" class="empty-state" style="display:none">
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"/></svg>
            <p>No rules found</p>
          </div>
        </div>
        <div id="rules-pagination" class="pagination" style="display:none"></div>
      </div>

      <!-- Connections View -->
      <div class="view" id="view-connections">
        <h2 class="mb-16">Connections</h2>
        <div class="card">
          <div class="stats-bar" id="conn-stats">
            <div class="stat-item"><span class="stat-label">Total:</span><span class="stat-value" id="conn-total">0</span></div>
            <div class="stat-item"><span class="stat-label">Upload:</span><span class="stat-value mono" id="conn-upload">0 B/s</span></div>
            <div class="stat-item"><span class="stat-label">Download:</span><span class="stat-value mono" id="conn-download">0 B/s</span></div>
            <div class="stat-item" style="margin-left:auto"><span class="stat-label">Refresh in:</span><span class="stat-value" id="conn-countdown">5s</span></div>
          </div>
        </div>
        <div class="card" style="padding:0;overflow:hidden">
          <div id="conn-loading" class="loading"><div class="spinner"></div>Loading connections...</div>
          <table class="table" id="conn-table" style="display:none">
            <thead><tr><th style="width:50px">#</th><th>Source</th><th>Target</th><th>Proxy</th><th style="width:90px">Upload</th><th style="width:90px">Download</th><th style="width:70px">Duration</th><th style="width:70px">Op</th></tr></thead>
            <tbody id="conn-list"></tbody>
          </table>
          <div id="conn-empty" class="empty-state" style="display:none">
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"/></svg>
            <p>No active connections</p>
          </div>
        </div>
      </div>

      <!-- Settings View -->
      <div class="view" id="view-settings">
        <h2 class="mb-16">Settings</h2>
        <div style="display:grid;gap:24px;max-width:600px">
          <div class="card">
            <div class="card-title">Service Operations</div>
            <div class="btn-group">
              <button class="btn" onclick="restartService()">Restart Service</button>
              <button class="btn danger" onclick="stopService()">Stop Service</button>
            </div>
          </div>
          <div class="card">
            <div class="card-title">Port Information</div>
            <div class="flex flex-col gap-8">
              <div class="flex justify-between"><span class="text2">HTTP Port</span><span class="mono" id="port-http">7890</span></div>
              <div class="flex justify-between"><span class="text2">SOCKS5 Port</span><span class="mono" id="port-socks5">7891</span></div>
              <div class="flex justify-between"><span class="text2">API Port</span><span class="mono" id="port-api">9090</span></div>
            </div>
          </div>
          <div class="card">
            <div class="card-title">About</div>
            <div class="flex flex-col gap-8">
              <div class="flex justify-between"><span class="text2">Version</span><span class="mono">0.1.0</span></div>
              <div class="flex justify-between"><span class="text2">Build</span><span class="mono">release</span></div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </main>
</div>

<!-- Toast -->
<div class="toast" id="toast"></div>

<!-- Delete Confirmation Modal -->
<div class="modal" id="deleteModal">
  <div class="modal-content">
    <h3 style="margin-bottom:16px">Confirm Delete</h3>
    <p id="deleteModalText" style="color:var(--text2);margin-bottom:24px">Are you sure you want to delete this item?</p>
    <div class="btn-group" style="justify-content:flex-end">
      <button class="btn secondary" onclick="closeDeleteModal()">Cancel</button>
      <button class="btn danger" id="deleteModalBtn">Delete</button>
    </div>
  </div>
</div>

<script>
// State
let currentView = 'dashboard';
let proxies = [];
let filteredProxies = [];
let profiles = [];
let currentProfileUid = null;
let rules = [];
let rulesPage = 1;
const rulesPerPage = 50;
let connections = [];
let connRefreshInterval = null;
let connCountdown = 5;
let connPaused = false;

// API helper
async function api(method, path, body) {
  const opts = { method, headers: {'Content-Type': 'application/json'} };
  if (body) opts.body = JSON.stringify(body);
  const res = await fetch(path, opts);
  const json = await res.json();
  if (json.code !== 0) throw new Error(json.message);
  return json.data;
}

// Toast
function showToast(msg, type = 'success') {
  const t = document.getElementById('toast');
  t.textContent = msg;
  t.className = `toast ${type} show`;
  setTimeout(() => t.classList.remove('show'), 3000);
}

// Navigation
document.querySelectorAll('.nav-item').forEach(el => {
  el.addEventListener('click', () => {
    const view = el.dataset.view;
    switchView(view);
  });
});

function switchView(view) {
  currentView = view;
  document.querySelectorAll('.nav-item').forEach(el => {
    el.classList.toggle('active', el.dataset.view === view);
  });
  document.querySelectorAll('.view').forEach(el => {
    el.classList.toggle('active', el.id === `view-${view}`);
  });

  // Load data for view
  if (view === 'dashboard') loadDashboard();
  else if (view === 'proxies') loadProxies();
  else if (view === 'profiles') loadProfiles();
  else if (view === 'rules') loadRules();
  else if (view === 'connections') startConnections();
  else stopConnections();
}

// Format bytes
function formatBytes(b) {
  if (b === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(b) / Math.log(k));
  return parseFloat((b / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

// Format duration
function formatDuration(s) {
  if (s < 60) return s + 's';
  if (s < 3600) return Math.floor(s / 60) + 'm ' + (s % 60) + 's';
  return Math.floor(s / 3600) + 'h ' + Math.floor((s % 3600) / 60) + 'm';
}

// Dashboard
async function loadDashboard() {
  try {
    const status = await api('GET', '/api/status');
    const mode = await api('GET', '/api/mode');

    document.getElementById('headerStatus').className = 'status-dot' + (status.running ? '' : ' stopped');
    document.getElementById('dash-status').textContent = status.running ? 'Running' : 'Stopped';
    document.getElementById('dash-uptime').textContent = status.running && status.uptime_secs
      ? 'Uptime: ' + formatDuration(status.uptime_secs)
      : '';
    document.getElementById('dash-mode').textContent = mode ? mode.toUpperCase() : 'Unknown';

    document.getElementById('btn-start').disabled = status.running;
    document.getElementById('btn-stop').disabled = !status.running;

    // Get proxy info from proxies
    try {
      const proxyData = await api('GET', '/api/proxies');
      const proxyNames = Object.keys(proxyData.proxies || {});
      const globalProxy = proxyData.proxies?.GLOBAL;
      const currentProxy = globalProxy?.now || '-';
      document.getElementById('dash-proxy').textContent = currentProxy;

      // Count online nodes (proxies that are not DIRECT or REJECT)
      const onlineNodes = proxyNames.filter(n => !['GLOBAL', 'DIRECT', 'REJECT', 'FALLBACK'].includes(n)).length;
      document.getElementById('dash-nodes').textContent = onlineNodes;
    } catch (e) {
      document.getElementById('dash-proxy').textContent = '-';
      document.getElementById('dash-nodes').textContent = '-';
    }

    // Get connections count
    try {
      const connData = await api('GET', '/api/connections');
      const count = (connData.connections || []).length;
      document.getElementById('dash-connections').textContent = count;
    } catch (e) {
      document.getElementById('dash-connections').textContent = '-';
    }
  } catch (e) {
    showToast('Failed to load dashboard: ' + e.message, 'error');
  }
}

async function startService() {
  try {
    await api('POST', '/api/service/start');
    showToast('Service started');
    loadDashboard();
  } catch (e) {
    showToast('Failed to start: ' + e.message, 'error');
  }
}

async function stopService() {
  try {
    await api('POST', '/api/service/stop');
    showToast('Service stopped');
    loadDashboard();
  } catch (e) {
    showToast('Failed to stop: ' + e.message, 'error');
  }
}

async function setMode(mode) {
  try {
    await api('POST', '/api/mode', { mode });
    showToast('Mode set to ' + mode);
    loadDashboard();
  } catch (e) {
    showToast('Failed to set mode: ' + e.message, 'error');
  }
}

// Proxies
async function loadProxies() {
  const loading = document.getElementById('proxy-loading');
  const table = document.getElementById('proxy-table');
  const empty = document.getElementById('proxy-empty');

  loading.style.display = 'block';
  table.style.display = 'none';
  empty.style.display = 'none';

  try {
    const data = await api('GET', '/api/proxies');
    proxies = [];

    // Parse Mihomo proxy format
    const proxyList = data.proxies || {};
    const globalNow = proxyList.GLOBAL?.now || '';

    for (const [name, info] of Object.entries(proxyList)) {
      if (['GLOBAL', 'DIRECT', 'REJECT', 'FALLBACK'].includes(name)) continue;
      proxies.push({
        name,
        type: info.type || 'unknown',
        latency: null,
        selected: name === globalNow
      });
    }

    // Get latency for each proxy
    const latencyPromises = proxies.map(async (p) => {
      try {
        const delayData = await api('GET', `/api/proxies/${encodeURIComponent(p.name)}/delay?timeout=5000`);
        p.latency = delayData.delay;
      } catch (e) {
        p.latency = null;
      }
    });

    await Promise.allSettled(latencyPromises);

    // Sort by latency (null at end)
    proxies.sort((a, b) => {
      if (a.latency === null && b.latency === null) return a.name.localeCompare(b.name);
      if (a.latency === null) return 1;
      if (b.latency === null) return -1;
      return a.latency - b.latency;
    });

    filteredProxies = [...proxies];
    renderProxies();
  } catch (e) {
    loading.style.display = 'none';
    empty.style.display = 'block';
    showToast('Failed to load proxies: ' + e.message, 'error');
  }
}

function filterProxies() {
  const q = document.getElementById('proxy-search').value.toLowerCase();
  filteredProxies = proxies.filter(p => p.name.toLowerCase().includes(q));
  renderProxies();
}

function renderProxies() {
  const loading = document.getElementById('proxy-loading');
  const table = document.getElementById('proxy-table');
  const empty = document.getElementById('proxy-empty');

  if (filteredProxies.length === 0) {
    loading.style.display = 'none';
    table.style.display = 'none';
    empty.style.display = 'block';
    return;
  }

  loading.style.display = 'none';
  table.style.display = 'table';
  empty.style.display = 'none';

  const tbody = document.getElementById('proxy-list');
  tbody.innerHTML = filteredProxies.map((p, i) => {
    const latencyClass = p.latency === null ? 'latency-timeout'
      : p.latency < 100 ? 'latency-good'
      : p.latency < 300 ? 'latency-medium'
      : 'latency-bad';
    const latencyText = p.latency === null ? 'Timeout' : p.latency + 'ms';
    const selectedBadge = p.selected ? '<span class="badge info">Selected</span>' : '';

    return `<tr onclick="selectProxy('${escapeHtml(p.name)}')" style="cursor:pointer">
      <td><span class="mono">${escapeHtml(p.name)}</span></td>
      <td>${escapeHtml(p.type)}</td>
      <td class="${latencyClass} mono">${latencyText}</td>
      <td>${selectedBadge}</td>
    </tr>`;
  }).join('');
}

async function selectProxy(name) {
  try {
    await api('POST', '/api/proxies/select', { name });
    showToast(`Selected: ${name}`);
    loadProxies();
    loadDashboard();
  } catch (e) {
    showToast('Failed to select proxy: ' + e.message, 'error');
  }
}

function escapeHtml(s) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

// Profiles
async function loadProfiles() {
  const loading = document.getElementById('profiles-loading');
  const table = document.getElementById('profiles-table');
  const empty = document.getElementById('profiles-empty');

  loading.style.display = 'block';
  table.style.display = 'none';
  empty.style.display = 'none';

  try {
    const data = await api('GET', '/api/profiles');
    profiles = data || [];
    renderProfiles();
  } catch (e) {
    loading.style.display = 'none';
    empty.style.display = 'block';
    showToast('Failed to load profiles: ' + e.message, 'error');
  }
}

function renderProfiles() {
  const loading = document.getElementById('profiles-loading');
  const table = document.getElementById('profiles-table');
  const empty = document.getElementById('profiles-empty');

  if (profiles.length === 0) {
    loading.style.display = 'none';
    table.style.display = 'none';
    empty.style.display = 'block';
    return;
  }

  loading.style.display = 'none';
  table.style.display = 'table';
  empty.style.display = 'none';

  // Get current profile from config
  api('GET', '/api/config').then(config => {
    currentProfileUid = config?.current_profile || null;
    renderProfileRows();
  }).catch(() => {
    currentProfileUid = null;
    renderProfileRows();
  });
}

function renderProfileRows() {
  const tbody = document.getElementById('profiles-list');
  tbody.innerHTML = profiles.map(p => {
    const isActive = p.uid === currentProfileUid;
    const status = isActive
      ? '<span class="badge success">Active</span>'
      : '<span class="badge">Inactive</span>';
    const activeClass = isActive ? 'profile-active' : '';

    return `<tr class="${activeClass}">
      <td>${escapeHtml(p.name || 'Unnamed')}</td>
      <td class="truncate" title="${escapeHtml(p.url || '')}">${escapeHtml(p.url || '-')}</td>
      <td>${status}</td>
      <td>
        <div class="btn-group" style="gap:4px">
          ${!isActive ? `<button class="btn sm success" onclick="activateProfile('${p.uid}')">Activate</button>` : ''}
          <button class="btn sm danger" onclick="deleteProfile('${p.uid}')">Delete</button>
        </div>
      </td>
    </tr>`;
  }).join('');
}

async function addProfile(e) {
  e.preventDefault();
  const url = document.getElementById('profile-url').value;
  const name = document.getElementById('profile-name').value;

  try {
    await api('POST', '/api/profiles', { url, name: name || undefined });
    showToast('Profile added');
    document.getElementById('profile-url').value = '';
    document.getElementById('profile-name').value = '';
    loadProfiles();
  } catch (e) {
    showToast('Failed to add profile: ' + e.message, 'error');
  }
}

async function activateProfile(uid) {
  try {
    await api('POST', `/api/profiles/${uid}/activate`);
    showToast('Profile activated');
    loadProfiles();
  } catch (e) {
    showToast('Failed to activate profile: ' + e.message, 'error');
  }
}

let deleteTarget = null;
let deleteType = null;

function deleteProfile(uid) {
  deleteTarget = uid;
  deleteType = 'profile';
  document.getElementById('deleteModalText').textContent = 'Are you sure you want to delete this profile?';
  document.getElementById('deleteModal').classList.add('show');
}

document.getElementById('deleteModalBtn').addEventListener('click', async () => {
  if (!deleteTarget) return;
  closeDeleteModal();

  if (deleteType === 'profile') {
    try {
      await api('DELETE', `/api/profiles/${deleteTarget}`);
      showToast('Profile deleted');
      loadProfiles();
    } catch (e) {
      showToast('Failed to delete: ' + e.message, 'error');
    }
  }
});

function closeDeleteModal() {
  document.getElementById('deleteModal').classList.remove('show');
  deleteTarget = null;
  deleteType = null;
}

// Rules
async function loadRules() {
  const loading = document.getElementById('rules-loading');
  const table = document.getElementById('rules-table');
  const empty = document.getElementById('rules-empty');

  loading.style.display = 'block';
  table.style.display = 'none';
  empty.style.display = 'none';

  try {
    // Try to get rules from config
    const config = await api('GET', '/api/config');
    rules = config?.rules || [];
    renderRules();
  } catch (e) {
    // Rules API not available
    rules = [];
    loading.style.display = 'none';
    empty.style.display = 'block';
    document.getElementById('rules-empty').querySelector('p').textContent = 'Rules API not available';
  }
}

function renderRules() {
  const loading = document.getElementById('rules-loading');
  const table = document.getElementById('rules-table');
  const empty = document.getElementById('rules-empty');
  const pagination = document.getElementById('rules-pagination');

  if (rules.length === 0) {
    loading.style.display = 'none';
    table.style.display = 'none';
    empty.style.display = 'block';
    pagination.style.display = 'none';
    return;
  }

  loading.style.display = 'none';
  table.style.display = 'table';
  empty.style.display = 'none';

  const totalPages = Math.ceil(rules.length / rulesPerPage);
  const start = (rulesPage - 1) * rulesPerPage;
  const pageRules = rules.slice(start, start + rulesPerPage);

  const tbody = document.getElementById('rules-list');
  tbody.innerHTML = pageRules.map((r, i) => {
    const idx = start + i + 1;
    const rule = Array.isArray(r) ? r : ['', '', ''];
    return `<tr>
      <td class="mono">${idx}</td>
      <td>${escapeHtml(rule[0] || '')}</td>
      <td class="truncate" style="max-width:300px" title="${escapeHtml(rule[1] || '')}">${escapeHtml(rule[1] || '')}</td>
      <td>${escapeHtml(rule[2] || '')}</td>
      <td><button class="btn sm danger" onclick="deleteRule(${start + i})">X</button></td>
    </tr>`;
  }).join('');

  // Pagination
  if (totalPages > 1) {
    pagination.style.display = 'flex';
    pagination.innerHTML = `
      <button class="page-btn" onclick="changeRulesPage(${rulesPage - 1})" ${rulesPage === 1 ? 'disabled' : ''}>Prev</button>
      <span class="page-info">Page ${rulesPage} of ${totalPages}</span>
      <button class="page-btn" onclick="changeRulesPage(${rulesPage + 1})" ${rulesPage === totalPages ? 'disabled' : ''}>Next</button>
    `;
  } else {
    pagination.style.display = 'none';
  }
}

function changeRulesPage(p) {
  rulesPage = p;
  renderRules();
}

async function addRule(e) {
  e.preventDefault();
  showToast('Rules API not implemented - changes will not persist', 'error');
  // Reset form
  document.getElementById('rule-value').value = '';
  document.getElementById('rule-proxy').value = '';
}

async function deleteRule(idx) {
  showToast('Rules API not implemented - changes will not persist', 'error');
}

// Connections
function startConnections() {
  connPaused = false;
  connCountdown = 5;
  loadConnections();
  if (connRefreshInterval) clearInterval(connRefreshInterval);
  connRefreshInterval = setInterval(() => {
    if (!connPaused) {
      connCountdown--;
      if (connCountdown <= 0) {
        connCountdown = 5;
        loadConnections();
      }
      document.getElementById('conn-countdown').textContent = connCountdown + 's';
    }
  }, 1000);
}

function stopConnections() {
  if (connRefreshInterval) {
    clearInterval(connRefreshInterval);
    connRefreshInterval = null;
  }
}

async function loadConnections() {
  const loading = document.getElementById('conn-loading');
  const table = document.getElementById('conn-table');
  const empty = document.getElementById('conn-empty');

  try {
    const data = await api('GET', '/api/connections');
    connections = data.connections || [];

    // Calculate totals
    let uploadTotal = 0, downloadTotal = 0;
    connections.forEach(c => {
      uploadTotal += c.upload || 0;
      downloadTotal += c.download || 0;
    });

    document.getElementById('conn-total').textContent = connections.length;
    document.getElementById('conn-upload').textContent = formatBytes(uploadTotal) + '/s';
    document.getElementById('conn-download').textContent = formatBytes(downloadTotal) + '/s';

    if (connections.length === 0) {
      loading.style.display = 'none';
      table.style.display = 'none';
      empty.style.display = 'block';
      return;
    }

    loading.style.display = 'none';
    table.style.display = 'table';
    empty.style.display = 'none';

    const tbody = document.getElementById('conn-list');
    tbody.innerHTML = connections.map((c, i) => `
      <tr onmouseenter="connPaused=true" onmouseleave="connPaused=false">
        <td class="mono">${i + 1}</td>
        <td class="mono" title="${escapeHtml(c.metadata?.source || '')}">${escapeHtml(truncate(c.metadata?.source || '-', 20))}</td>
        <td class="mono" title="${escapeHtml(c.metadata?.target || '')}">${escapeHtml(truncate(c.metadata?.target || '-', 25))}</td>
        <td>${escapeHtml(c.metadata?.proxy || '-')}</td>
        <td class="mono">${formatBytes(c.upload || 0)}</td>
        <td class="mono">${formatBytes(c.download || 0)}</td>
        <td class="mono">${formatDuration(c.duration || 0)}</td>
        <td><button class="btn sm danger" onclick="closeConnection('${escapeHtml(c.id)}')">X</button></td>
      </tr>
    `).join('');
  } catch (e) {
    loading.style.display = 'none';
    table.style.display = 'none';
    empty.style.display = 'block';
    empty.querySelector('p').textContent = 'Failed to load connections';
  }
}

function truncate(s, len) {
  return s && s.length > len ? s.slice(0, len) + '...' : s;
}

async function closeConnection(id) {
  try {
    await api('DELETE', `/api/connections/${encodeURIComponent(id)}`);
    showToast('Connection closed');
    loadConnections();
  } catch (e) {
    showToast('Failed to close connection: ' + e.message, 'error');
  }
}

// Settings
async function restartService() {
  try {
    await api('POST', '/api/service/stop');
    await new Promise(r => setTimeout(r, 1000));
    await api('POST', '/api/service/start');
    showToast('Service restarted');
    loadDashboard();
  } catch (e) {
    showToast('Failed to restart: ' + e.message, 'error');
  }
}

// Initialize
document.addEventListener('DOMContentLoaded', () => {
  loadDashboard();
});
</script>
</body>
</html>
"#;
