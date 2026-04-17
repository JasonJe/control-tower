//! Embedded HTML/CSS/JS for Web UI

pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Control Tower</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1a1a2e;
            color: #eee;
            min-height: 100vh;
        }
        .container {
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
        }
        header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 20px;
            background: #16213e;
            border-radius: 8px;
            margin-bottom: 20px;
        }
        header h1 {
            color: #00d9ff;
        }
        .status {
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .status-dot {
            width: 12px;
            height: 12px;
            border-radius: 50%;
            background: #666;
        }
        .status-dot.running {
            background: #00ff88;
        }
        .status-dot.stopped {
            background: #ff4444;
        }
        .nav {
            display: flex;
            gap: 10px;
            margin-bottom: 20px;
        }
        .nav-btn {
            padding: 10px 20px;
            background: #16213e;
            border: none;
            border-radius: 6px;
            color: #eee;
            cursor: pointer;
            transition: background 0.2s;
        }
        .nav-btn:hover, .nav-btn.active {
            background: #0f3460;
        }
        .card {
            background: #16213e;
            border-radius: 8px;
            padding: 20px;
            margin-bottom: 20px;
        }
        .card h2 {
            margin-bottom: 15px;
            color: #00d9ff;
        }
        .form-group {
            margin-bottom: 15px;
        }
        .form-group label {
            display: block;
            margin-bottom: 5px;
            color: #aaa;
        }
        .form-group input {
            width: 100%;
            padding: 10px;
            border: 1px solid #333;
            border-radius: 4px;
            background: #0f3460;
            color: #eee;
        }
        .btn {
            padding: 10px 20px;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 14px;
        }
        .btn-primary {
            background: #00d9ff;
            color: #000;
        }
        .btn-danger {
            background: #ff4444;
            color: #fff;
        }
        .btn-success {
            background: #00ff88;
            color: #000;
        }
        .profile-list {
            list-style: none;
        }
        .profile-item {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 15px;
            background: #0f3460;
            border-radius: 6px;
            margin-bottom: 10px;
        }
        .profile-item.active {
            border: 2px solid #00ff88;
        }
        .proxy-list {
            display: grid;
            grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
            gap: 10px;
        }
        .proxy-item {
            padding: 15px;
            background: #0f3460;
            border-radius: 6px;
            cursor: pointer;
            transition: background 0.2s;
        }
        .proxy-item:hover {
            background: #1a4a80;
        }
        .proxy-item.selected {
            border: 2px solid #00d9ff;
        }
        .mode-selector {
            display: flex;
            gap: 10px;
        }
        .mode-btn {
            flex: 1;
            padding: 15px;
            background: #0f3460;
            border: 2px solid transparent;
            border-radius: 6px;
            color: #eee;
            cursor: pointer;
            text-align: center;
        }
        .mode-btn.active {
            border-color: #00d9ff;
            background: #1a4a80;
        }
        .connection-item {
            display: flex;
            justify-content: space-between;
            padding: 10px;
            background: #0f3460;
            border-radius: 4px;
            margin-bottom: 5px;
            font-size: 14px;
        }
        .loading {
            text-align: center;
            padding: 40px;
            color: #aaa;
        }
        .error {
            color: #ff4444;
            padding: 10px;
            background: rgba(255, 68, 68, 0.1);
            border-radius: 4px;
            margin-bottom: 10px;
        }
        .success {
            color: #00ff88;
            padding: 10px;
            background: rgba(0, 255, 136, 0.1);
            border-radius: 4px;
            margin-bottom: 10px;
        }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>Control Tower</h1>
            <div class="status">
                <span class="status-dot" id="statusDot"></span>
                <span id="statusText">Checking...</span>
            </div>
        </header>

        <nav class="nav">
            <button class="nav-btn active" data-view="profiles">Profiles</button>
            <button class="nav-btn" data-view="proxies">Proxies</button>
            <button class="nav-btn" data-view="connections">Connections</button>
            <button class="nav-btn" data-view="settings">Settings</button>
        </nav>

        <!-- Profiles View -->
        <div id="profilesView" class="view">
            <div class="card">
                <h2>Add Profile</h2>
                <div class="form-group">
                    <label>Subscription URL</label>
                    <input type="text" id="profileUrl" placeholder="https://example.com/sub.yaml">
                </div>
                <div class="form-group">
                    <label>Name (optional)</label>
                    <input type="text" id="profileName" placeholder="My Proxy">
                </div>
                <button class="btn btn-primary" onclick="addProfile()">Add Profile</button>
                <div id="profileMsg"></div>
            </div>

            <div class="card">
                <h2>Profiles</h2>
                <div id="profileList">
                    <div class="loading">Loading...</div>
                </div>
            </div>
        </div>

        <!-- Proxies View -->
        <div id="proxiesView" class="view" style="display:none;">
            <div class="card">
                <h2>Mode</h2>
                <div class="mode-selector">
                    <button class="mode-btn" data-mode="rule" onclick="setMode('rule')">Rule</button>
                    <button class="mode-btn" data-mode="global" onclick="setMode('global')">Global</button>
                    <button class="mode-btn" data-mode="direct" onclick="setMode('direct')">Direct</button>
                </div>
            </div>

            <div class="card">
                <h2>Proxy Nodes</h2>
                <div id="proxyList">
                    <div class="loading">Loading...</div>
                </div>
            </div>
        </div>

        <!-- Connections View -->
        <div id="connectionsView" class="view" style="display:none;">
            <div class="card">
                <h2>Active Connections</h2>
                <div id="connectionList">
                    <div class="loading">Loading...</div>
                </div>
            </div>
        </div>

        <!-- Settings View -->
        <div id="settingsView" class="view" style="display:none;">
            <div class="card">
                <h2>Service</h2>
                <div id="serviceStatus">Checking...</div>
                <button class="btn btn-success" onclick="startService()">Start</button>
                <button class="btn btn-danger" onclick="stopService()">Stop</button>
            </div>

            <div class="card">
                <h2>Configuration</h2>
                <div id="configContent">
                    <div class="loading">Loading...</div>
                </div>
            </div>
        </div>
    </div>

    <script>
        const API_BASE = '/api';
        let currentMode = 'rule';

        // Navigation
        document.querySelectorAll('.nav-btn').forEach(btn => {
            btn.addEventListener('click', () => {
                document.querySelectorAll('.nav-btn').forEach(b => b.classList.remove('active'));
                btn.classList.add('active');

                document.querySelectorAll('.view').forEach(v => v.style.display = 'none');
                document.getElementById(btn.dataset.view + 'View').style.display = 'block';

                if (btn.dataset.view === 'proxies') loadProxies();
                if (btn.dataset.view === 'connections') loadConnections();
                if (btn.dataset.view === 'settings') loadSettings();
            });
        });

        // API helpers
        async function api(path, options = {}) {
            try {
                const res = await fetch(API_BASE + path, options);
                return await res.json();
            } catch (e) {
                return { code: -1, message: e.message };
            }
        }

        function showMsg(id, msg, type = 'error') {
            const el = document.getElementById(id);
            el.className = type;
            el.textContent = msg;
            setTimeout(() => el.textContent = '', 3000);
        }

        // Status
        async function checkStatus() {
            const res = await api('/service/status');
            if (res.code === 0 && res.data) {
                const dot = document.getElementById('statusDot');
                const text = document.getElementById('statusText');
                if (res.data.running) {
                    dot.className = 'status-dot running';
                    text.textContent = 'Running (' + res.data.mode + ')';
                    currentMode = res.data.mode;
                    updateModeButtons();
                } else {
                    dot.className = 'status-dot stopped';
                    text.textContent = 'Stopped';
                }
            }
        }

        // Profiles
        async function loadProfiles() {
            const res = await api('/profiles');
            const el = document.getElementById('profileList');

            if (res.code !== 0 || !res.data || res.data.length === 0) {
                el.innerHTML = '<p>No profiles found.</p>';
                return;
            }

            el.innerHTML = '<ul class="profile-list">' + res.data.map(p => `
                <li class="profile-item ${p.active ? 'active' : ''}">
                    <div>
                        <strong>${p.name}</strong>
                        ${p.active ? '<span style="color:#00ff88"> (Active)</span>' : ''}
                    </div>
                    <div>
                        ${!p.active ? '<button class="btn btn-success" onclick="activateProfile(\'' + p.id + '\')">Activate</button>' : ''}
                        <button class="btn btn-danger" onclick="deleteProfile(\'' + p.id + '\')">Delete</button>
                    </div>
                </li>
            `).join('') + '</ul>';
        }

        async function addProfile() {
            const url = document.getElementById('profileUrl').value;
            const name = document.getElementById('profileName').value;

            if (!url) {
                showMsg('profileMsg', 'URL is required');
                return;
            }

            const res = await api('/profiles', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ url, name })
            });

            if (res.code === 0) {
                showMsg('profileMsg', 'Profile added successfully', 'success');
                document.getElementById('profileUrl').value = '';
                document.getElementById('profileName').value = '';
                loadProfiles();
            } else {
                showMsg('profileMsg', res.message);
            }
        }

        async function activateProfile(id) {
            const res = await api('/profiles/' + id + '/activate', { method: 'POST' });
            if (res.code === 0) {
                loadProfiles();
            }
        }

        async function deleteProfile(id) {
            if (!confirm('Delete this profile?')) return;
            const res = await api('/profiles/' + id, { method: 'DELETE' });
            if (res.code === 0) {
                loadProfiles();
            }
        }

        // Mode
        function updateModeButtons() {
            document.querySelectorAll('.mode-btn').forEach(btn => {
                btn.classList.toggle('active', btn.dataset.mode === currentMode);
            });
        }

        async function setMode(mode) {
            const res = await api('/mode', {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ mode })
            });

            if (res.code === 0) {
                currentMode = mode;
                updateModeButtons();
            }
        }

        // Proxies
        async function loadProxies() {
            const res = await api('/proxies');
            const el = document.getElementById('proxyList');

            if (res.code !== 0 || !res.data || !res.data.proxies) {
                el.innerHTML = '<p>Service not running or no proxies available.</p>';
                return;
            }

            let html = '<div class="proxy-list">';
            for (const [name, list] of Object.entries(res.data.proxies)) {
                if (Array.isArray(list)) {
                    list.forEach(p => {
                        const latency = p.history && p.history[0] && p.history[0].delay;
                        html += `
                            <div class="proxy-item" onclick="selectProxy('${p.name}')">
                                <strong>${p.name}</strong>
                                <br><small>${p.type || 'unknown'}</small>
                                <br><small>${p.alive ? '● Alive' : '✗ Dead'} ${latency ? '- ' + latency + 'ms' : ''}</small>
                            </div>
                        `;
                    });
                }
            }
            html += '</div>';
            el.innerHTML = html;
        }

        async function selectProxy(name) {
            const res = await api('/proxies/select', {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name })
            });

            if (res.code === 0) {
                loadProxies();
            }
        }

        // Connections
        async function loadConnections() {
            const res = await api('/connections');
            const el = document.getElementById('connectionList');

            if (res.code !== 0 || !res.data || !res.data.connections) {
                el.innerHTML = '<p>No active connections.</p>';
                return;
            }

            const conns = res.data.connections;
            if (conns.length === 0) {
                el.innerHTML = '<p>No active connections.</p>';
                return;
            }

            el.innerHTML = conns.map(c => `
                <div class="connection-item">
                    <span>${c.metadata?.source || '?'} → ${c.metadata?.target || '?'}</span>
                    <button class="btn btn-danger" onclick="closeConnection('${c.id}')">Close</button>
                </div>
            `).join('');
        }

        async function closeConnection(id) {
            const res = await api('/connections/' + id, { method: 'DELETE' });
            if (res.code === 0) {
                loadConnections();
            }
        }

        // Settings
        async function loadSettings() {
            const status = await api('/service/status');
            const config = await api('/config');

            document.getElementById('serviceStatus').innerHTML = status.data
                ? `<p>Status: ${status.data.running ? 'Running' : 'Stopped'}</p><p>Mode: ${status.data.mode}</p>`
                : '<p>Error loading status</p>';

            document.getElementById('configContent').innerHTML = config.data
                ? '<pre>' + JSON.stringify(config.data, null, 2) + '</pre>'
                : '<p>No configuration</p>';
        }

        async function startService() {
            alert('Start service not implemented in web UI. Use CLI: clash service start');
        }

        async function stopService() {
            if (!confirm('Stop the service?')) return;
            const res = await api('/service/stop', { method: 'POST' });
            checkStatus();
        }

        // Init
        checkStatus();
        setInterval(checkStatus, 5000);
        loadProfiles();
    </script>
</body>
</html>"#;
