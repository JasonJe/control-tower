# Control Tower / 塔台

基于 [Mihomo](https://github.com/MetaCubeX/mihomo) 的代理管理工具，提供 CLI 和 Web UI，支持订阅自动更新、节点测速、流量统计等进阶功能。

---

## 特性

- **Web UI** — 嵌入式管理界面，HTTPS 加密传输，支持密码认证
- **订阅管理** — 多订阅源支持，自动更新（直连或走代理），可配置 cron 表达式
- **Profile 类型** — 支持 HTTP/订阅/script/merge/local 多种类型
- **Rule Providers** — 支持外部规则集，可视化管理 interval 和启用状态
- **DNS 独立配置** — 不依赖 TUN 开关，可独立配置 fake-ip、nameserver、hosts 等
- **连接历史** — 自动记录关闭的连接（来源/目标/节点/流量），持久化存储
- **节点测速** — 一键批量检测节点延迟
- **流量统计** — 实时上下行速率 + 累计流量
- **CLI 工具** — `ctctl` 提供完整的命令行操作能力

---

## 快速开始

### 构建

```bash
./build.sh
```

产物：`target/release/ctsvc`（服务）、`target/release/ctctl`（CLI）

### 安装服务

```bash
cd target/dist/control-tower
sudo ./install-service.sh
```

服务部署至 `/opt/ctsvc`，注册 systemd 单元 `ctsvc`。

### 启动与停止

```bash
sudo systemctl start ctsvc   # 启动
sudo systemctl stop ctsvc    # 停止
sudo systemctl status ctsvc   # 状态
```

### 访问 Web UI

浏览器打开 **https://127.0.0.1:8080**（首次需设置密码）

---

## CLI 参考

```bash
# 订阅管理
ctctl profile list                    # 列出所有订阅
ctctl profile add <URL> [名称]         # 添加订阅
ctctl profile add <URL> [名称] -H "X-Token: abc"  # 添加订阅并自定义请求头
ctctl profile activate <ID>            # 激活订阅
ctctl profile cron <ID> <分钟>        # 设置自动更新间隔
ctctl profile update                  # 手动更新（沿用已保存的请求头）
ctctl profile update <ID> -H "Authorization: Bearer abc"  # 覆盖请求头并更新

# 节点管理
ctctl proxy list                      # 列出节点
ctctl proxy select <名称>              # 选择节点
ctctl proxy test                      # 测速所有节点

# 模式切换
ctctl mode get                        # 查看当前模式
ctctl mode set global|rule|direct     # 切换模式

# 连接管理
ctctl connections list                 # 当前连接
ctctl connections close <ID>          # 关闭连接
ctctl connections history             # 历史记录

# 设置
ctctl dns get|set                     # DNS 配置
ctctl rule-provider list              # Rule Providers
```

---

## settings.yaml 示例

```yaml
api_host: 127.0.0.1
api_port: 29090
http_port: 27890
socks_port: 27891
service_port: 8080
tun_enabled: true
log_level: warning
mode: rule

https:
  enabled: true
  cert_path: /opt/ctsvc/cert.pem
  key_path: /opt/ctsvc/key.pem

auth:
  enabled: true
  password: <pbkdf2-hash>

dns:
  enable: true
  enhanced_mode: fake-ip
  fake_ip_range: 198.18.0.1/15
  nameserver:
    - https://doh.pub/dns-query
  fallback:
    - https://1.1.1.1/dns-query

auto-update-on-startup: true

rule-providers:
  - name: adblock_reject
    type: http
    behavior: domain
    url: https://...
    interval: 1200
    enabled: true
```

> 鉴权说明：
> - 只有 `auth.enabled: true` 且 `password` 非空时才会强制鉴权；缺少 `auth` 段或密码为空时 API 保持开放（删除 `auth` 段等价于关闭鉴权，请谨慎操作）。
> - 开启鉴权后，除 `/api/auth/*`（登录/状态/首次设置密码）外，所有 `/api/*` 接口都需要 `Authorization: Bearer <token>`。
> - 密码只能通过 Web UI 首次设置（`POST /api/auth/setup-password`），请求体是 PBKDF2 派生 key 而不是明文密码；首次设置只允许从本机（loopback）发起，已有密码时该接口会拒绝覆盖，重置请直接编辑 `settings.yaml`。
> - `GET /api/settings` 不会返回 `auth.password`；`GET /api/profiles` 不会返回 profile 的自定义请求头（可能含订阅 token）。
> - Web UI 提供 Sign Out 入口，登出会同时删除服务端 session 和本地 token。

---

## 架构

```
┌──────────────────────────────────┐
│         Web Browser               │
│    https://127.0.0.1:8080        │
└──────────────┬───────────────────┘
               │ HTTPS
┌──────────────▼───────────────────┐
│        ctsvc (HTTP Server)        │
│   actix-web · /api/* · HTML UI  │
└──────┬───────────────┬──────────┘
       │               │
   Unix Socket      HTTP
┌──────▼──────┐  ┌───▼────────────┐
│ ctctl (CLI) │  │ Mihomo (Subprocess) │
│  用户 CLI   │  │  Port 29090 API     │
└─────────────┘  └──────────────────┘
```

- **ctsvc** — 无头 HTTP 服务，提供 API 与 Web UI
- **ctctl** — 用户 CLI，通过 Unix Socket 与 ctsvc 通信
- **Mihomo** — 由 ctsvc 管理的子进程，提供代理能力