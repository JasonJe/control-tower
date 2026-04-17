# Control Tower Service API

## Base URL

```
http://127.0.0.1:8080/api
```

端口可通过 `settings.yaml` 中的 `service_port` 配置项进行修改，默认为 `8080`。

---

## 通用响应格式

所有 API 响应均采用 JSON 格式，外层结构如下：

```json
{
  "code": 0,
  "message": "success",
  "data": { ... }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `code` | integer | 状态码，`0` 表示成功，`-1` 表示失败 |
| `message` | string | 状态信息 |
| `data` | object/null | 响应数据，失败时为 `null` |

---

## Endpoints

### Service Status

#### GET /api/status

获取 Mihomo 服务状态。

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "running": true,
    "pid": 12345,
    "uptime_secs": 3600,
    "state": "Running",
    "config_path": "/home/user/.config/control-tower/active_config.yaml",
    "circuit_breaker_remaining_secs": null
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `running` | boolean | 服务是否运行中 |
| `pid` | number/null | Mihomo 进程 ID |
| `uptime_secs` | number/null | 服务运行时长（秒） |
| `state` | string | 服务状态：`Running`、`NotRunning`、`CircuitBroken` |
| `config_path` | string/null | 当前配置文件的路径 |
| `circuit_breaker_remaining_secs` | number/null | 熔断器剩余冷却时间（秒） |

---

### Service Control

#### POST /api/service/start

启动 Mihomo 服务。

**Request:** 无

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

#### POST /api/service/stop

停止 Mihomo 服务。

**Request:** 无

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

### Profiles

#### GET /api/profiles

获取所有配置列表。

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": [
    {
      "uid": "abc123",
      "name": "Profile 1",
      "file": "abc123.yaml",
      "url": "https://example.com/profile.yaml",
      "cron": null,
      "updated_at": 1713000000
    }
  ]
}
```

---

#### POST /api/profiles

添加新的远程配置。

**Request:**

```json
{
  "url": "https://example.com/profile.yaml",
  "name": "My Profile"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `url` | string | 是 | 配置文件 URL |
| `name` | string | 否 | 配置名称 |

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "uid": "abc123",
    "file": "/home/user/.config/control-tower/profiles/abc123.yaml"
  }
}
```

---

#### POST /api/profiles/{id}/activate

激活指定配置。

**Path Parameters:**

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | string | 配置 UID |

**Request:** 无

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

#### DELETE /api/profiles/{id}

删除指定配置。

**Path Parameters:**

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | string | 配置 UID |

**Request:** 无

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

### Proxy Mode

#### GET /api/mode

获取当前代理模式。

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": "rule"
}
```

返回值为字符串，可能的值：`rule`（规则模式）、`global`（全局模式）、`direct`（直连模式）。

---

#### POST /api/mode

设置代理模式。

**Request:**

```json
{
  "mode": "rule"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `mode` | string | 是 | 代理模式：`rule`、`global`、`direct` |

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

### Proxies

#### GET /api/proxies

获取 Mihomo 代理组信息。

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "proxies": { ... }
  }
}
```

返回 Mihomo 的完整代理组 JSON 结构。

---

#### POST /api/proxies/select

选择 GLOBAL 组中的代理。

**Request:**

```json
{
  "name": "Proxy-1"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 代理名称 |

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

#### GET /api/proxies/{name}/delay

测试指定代理的延迟。

**Path Parameters:**

| 参数 | 类型 | 说明 |
|------|------|------|
| `name` | string | 代理名称 |

**Query Parameters:**

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `timeout` | integer | 5000 | 超时时间（毫秒） |

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "delay": 120
  }
}
```

返回值为代理响应延迟（毫秒）。

---

### Connections

#### GET /api/connections

获取当前连接列表。

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "connections": [ ... ]
  }
}
```

返回 Mihomo 当前活动连接的 JSON 结构。

---

#### DELETE /api/connections/{id}

关闭指定连接。

**Path Parameters:**

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | string | 连接 ID |

**Request:** 无

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": null
}
```

---

### Configuration

#### GET /api/config

获取当前 verge.yaml 配置。

**Response:**

```json
{
  "code": 0,
  "message": "success",
  "data": { ... }
}
```

返回 verge.yaml 解析后的 JSON 结构。

---

## API 端点汇总

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/status` | 获取服务状态 |
| POST | `/api/service/start` | 启动 Mihomo 服务 |
| POST | `/api/service/stop` | 停止 Mihomo 服务 |
| GET | `/api/profiles` | 获取配置列表 |
| POST | `/api/profiles` | 添加新配置 |
| POST | `/api/profiles/{id}/activate` | 激活指定配置 |
| DELETE | `/api/profiles/{id}` | 删除指定配置 |
| GET | `/api/mode` | 获取代理模式 |
| POST | `/api/mode` | 设置代理模式 |
| GET | `/api/proxies` | 获取代理组信息 |
| POST | `/api/proxies/select` | 选择代理 |
| GET | `/api/proxies/{name}/delay` | 测试代理延迟 |
| GET | `/api/connections` | 获取连接列表 |
| DELETE | `/api/connections/{id}` | 关闭连接 |
| GET | `/api/config` | 获取配置文件 |

---

## 错误码

| code | 说明 |
|------|------|
| `0` | 成功 |
| `-1` | 通用错误（失败原因见 `message` 字段） |
