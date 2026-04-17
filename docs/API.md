# Control Tower Web API 文档

## 概述

Control Tower 提供基于 HTTP 的 REST API，用于 Web UI 开发和系统集成。

## 基础信息

| 项目 | 值 |
|------|-----|
| Base URL | `http://127.0.0.1:8080/api` |
| 响应格式 | JSON |
| 认证 | 无（仅本地访问） |

---

## 响应格式

### 成功响应
```json
{
  "code": 0,
  "message": "success",
  "data": { ... }
}
```

### 错误响应
```json
{
  "code": -1,
  "message": "错误描述",
  "data": null
}
```

---

## API 端点

### 健康检查

#### GET /
返回服务状态字符串

**请求**
```bash
curl http://127.0.0.1:8080/
```

**响应**
```json
"Control Tower API"
```

---

### 订阅管理

#### GET /profiles
获取所有订阅列表

**请求**
```bash
curl http://127.0.0.1:8080/api/profiles
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": [
    {
      "id": "12345678",
      "name": "我的订阅",
      "url": "https://example.com/sub.yaml",
      "file": "/root/.config/control-tower/profiles/12345678.yaml",
      "active": true
    }
  ]
}
```

---

#### POST /profiles
添加新订阅（下载并保存）

**请求体**
```json
{
  "url": "https://example.com/sub.yaml",
  "name": "可选的名称"
}
```

**请求示例**
```bash
curl -X POST http://127.0.0.1:8080/api/profiles \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com/sub.yaml", "name": "我的订阅"}'
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "id": "profile-550e8400-e29b-41d4-a716-446655440000",
    "name": "我的订阅",
    "url": "https://example.com/sub.yaml",
    "file": null,
    "active": false
  }
}
```

---

#### DELETE /profiles/{id}
删除订阅

**请求示例**
```bash
curl -X DELETE http://127.0.0.1:8080/api/profiles/profile-550e8400-e29b-41d4-a716-446655440000
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

**错误响应** (Profile not found)
```json
{
  "code": -1,
  "message": "Profile not found",
  "data": null
}
```

---

#### POST /profiles/{id}/activate
激活指定订阅

**请求示例**
```bash
curl -X POST http://127.0.0.1:8080/api/profiles/12345678/activate
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

### 代理管理

#### GET /proxies
获取所有代理节点（透传 Mihomo API）

**请求示例**
```bash
curl http://127.0.0.1:8080/api/proxies
```

**响应** (透传 Mihomo)
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "GLOBAL": {
      "name": "GLOBAL",
      "type": "Selector",
      "all": ["DIRECT", "REJECT", "香港 101", "..."],
      "now": "香港 101"
    },
    "香港 101": {
      "name": "香港 101",
      "type": "Vless",
      "alive": true,
      "history": []
    }
  }
}
```

---

#### PUT /proxies/select
选择代理节点

**请求体**
```json
{
  "name": "香港 101"
}
```

**请求示例**
```bash
curl -X PUT http://127.0.0.1:8080/api/proxies/select \
  -H "Content-Type: application/json" \
  -d '{"name": "香港 101"}'
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

### 连接管理

#### GET /connections
获取当前所有连接（透传 Mihomo API）

**请求示例**
```bash
curl http://127.0.0.1:8080/api/connections
```

**响应** (透传 Mihomo)
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "connections": [...],
    "uploadTotal": 10240,
    "downloadTotal": 20480
  }
}
```

---

#### DELETE /connections/{id}
关闭指定连接

**请求示例**
```bash
curl -X DELETE http://127.0.0.1:8080/api/connections/abc123-def456
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

### 模式管理

#### GET /mode
获取当前代理模式

**请求示例**
```bash
curl http://127.0.0.1:8080/api/mode
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": "rule"
}
```

**可能的值**: `"rule"` | `"global"` | `"direct"`

---

#### PUT /mode
设置代理模式

**请求体**
```json
{
  "mode": "global"
}
```

**请求示例**
```bash
curl -X PUT http://127.0.0.1:8080/api/mode \
  -H "Content-Type: application/json" \
  -d '{"mode": "global"}'
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

**错误响应** (无效模式)
```json
{
  "code": -1,
  "message": "Invalid mode",
  "data": null
}
```

---

### 服务状态

#### GET /service/status
获取服务运行状态

**请求示例**
```bash
curl http://127.0.0.1:8080/api/service/status
```

**响应** (服务运行中)
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "running": true,
    "mode": "rule"
  }
}
```

**响应** (服务已停止)
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "running": false,
    "mode": "stopped"
  }
}
```

---

### 配置管理

#### GET /config
获取 verge.yaml 配置

**请求示例**
```bash
curl http://127.0.0.1:8080/api/config
```

**响应**
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "port": 9090,
    "socks-port": 7890,
    ...
  }
}
```

---

## 端点汇总

| 方法 | 路径 | 描述 |
|------|------|------|
| GET | `/` | 健康检查 |
| GET | `/api/profiles` | 获取订阅列表 |
| POST | `/api/profiles` | 添加订阅 |
| DELETE | `/api/profiles/{id}` | 删除订阅 |
| POST | `/api/profiles/{id}/activate` | 激活订阅 |
| GET | `/api/proxies` | 获取代理列表 |
| PUT | `/api/proxies/select` | 选择代理 |
| GET | `/api/connections` | 获取连接列表 |
| DELETE | `/api/connections/{id}` | 关闭连接 |
| GET | `/api/mode` | 获取模式 |
| PUT | `/api/mode` | 设置模式 |
| GET | `/api/service/status` | 服务状态 |
| GET | `/api/config` | 获取配置 |

---

## Web UI

Web UI 提供图形化界面管理代理。

**访问地址**: http://127.0.0.1:8080

**启动 Web UI**:
```bash
control-tower web
```

**自定义端口**:
```bash
control-tower web --port 9000
```

---

## 前端集成示例

### JavaScript (Fetch)

```javascript
const API_BASE = 'http://127.0.0.1:8080/api';

// 获取代理列表
async function getProxies() {
  const res = await fetch(`${API_BASE}/proxies`);
  const data = await res.json();
  if (data.code === 0) return data.data;
  throw new Error(data.message);
}

// 选择代理
async function selectProxy(name) {
  const res = await fetch(`${API_BASE}/proxies/select`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name })
  });
  const data = await res.json();
  if (data.code !== 0) throw new Error(data.message);
}

// 获取连接
async function getConnections() {
  const res = await fetch(`${API_BASE}/connections`);
  const data = await res.json();
  if (data.code === 0) return data.data;
  throw new Error(data.message);
}

// 关闭连接
async function closeConnection(id) {
  const res = await fetch(`${API_BASE}/connections/${id}`, {
    method: 'DELETE'
  });
  const data = await res.json();
  if (data.code !== 0) throw new Error(data.message);
}

// 获取订阅
async function getProfiles() {
  const res = await fetch(`${API_BASE}/profiles`);
  const data = await res.json();
  if (data.code === 0) return data.data;
  throw new Error(data.message);
}

// 添加订阅
async function addProfile(url, name) {
  const res = await fetch(`${API_BASE}/profiles`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url, name })
  });
  const data = await res.json();
  if (data.code !== 0) throw new Error(data.message);
  return data.data;
}

// 获取/设置模式
async function getMode() {
  const res = await fetch(`${API_BASE}/mode`);
  const data = await res.json();
  if (data.code === 0) return data.data;
  throw new Error(data.message);
}

async function setMode(mode) {
  const res = await fetch(`${API_BASE}/mode`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ mode })
  });
  const data = await res.json();
  if (data.code !== 0) throw new Error(data.message);
}
```

### React 组件示例

```jsx
import React, { useState, useEffect } from 'react';

function ProxySelector() {
  const [proxies, setProxies] = useState([]);
  const [selected, setSelected] = useState('');

  useEffect(() => {
    fetch('http://127.0.0.1:8080/api/proxies')
      .then(r => r.json())
      .then(data => {
        if (data.code === 0) {
          const globalProxy = data.data.GLOBAL;
          setProxies(globalProxy?.all || []);
          setSelected(globalProxy?.now || '');
        }
      });
  }, []);

  const handleSelect = async (name) => {
    await fetch('http://127.0.0.1:8080/api/proxies/select', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name })
    });
    setSelected(name);
  };

  return (
    <select value={selected} onChange={e => handleSelect(e.target.value)}>
      {proxies.map(p => (
        <option key={p} value={p}>{p}</option>
      ))}
    </select>
  );
}
```

---

## 注意事项

1. **错误处理**: 务必检查 `code` 字段，`0` 表示成功，`-1` 表示失败
2. **CORS**: API 仅供本地访问，不存在跨域问题
3. **服务依赖**: 部分 API 需要 Mihomo 服务运行才能正常工作
4. **数据透传**: `/api/proxies` 和 `/api/connections` 是 Mihomo API 的透传，响应格式与 Mihomo 保持一致
