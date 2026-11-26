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
- **系统代理** — 一键开启/关闭 Linux 桌面代理，服务重启后自动恢复
- **策略组** — 按策略组选择节点并持久化，切换订阅/重启后保留选择
- **订阅信息** — `ctctl profile list` 展示订阅流量与到期时间（`subscription-userinfo`）
- **分享链接导入** — 粘贴 vmess/vless/trojan/ss 链接直接生成本地 profile
- **内核看护** — Mihomo 异常退出自动重启（指数退避），支持 `ctctl service install`
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

浏览器打开 **http://127.0.0.1:8080**（启用 HTTPS 后为 `https://127.0.0.1:8080`；首次需设置密码）

---

## CLI 参考

> `ctctl` 需要以服务运行用户（通过 installer 安装时通常是 `root`）执行，才能访问权限为 `0600` 的 IPC socket 和配置目录；普通用户可使用 Web UI 或只读接口。

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
ctctl service install                 # 安装 systemd 服务（需 root）
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

> 端口说明：
> - `http_port` 对应 Mihomo 的 `port`（独立的 HTTP 代理端口），`socks_port` 对应 `socks-port`；两者互不影响，`http_port` 不会再被写成 `mixed-port`。
> - 如需启用 mixed 端口，请在 settings.yaml 中显式添加 `mixed_port: <port>`。若它与 `http_port`/`socks_port` 冲突，生成的 config.yaml 会移除 `mixed-port` 并记录告警（HTTP/SOCKS 端口优先）。
> - 在 Web UI 修改端口后会立即重写 config.yaml 并重启 Mihomo；HTTP 端口与 SOCKS 端口必须不同，且不能与 Mihomo API 端口或 Control Tower 服务端口冲突。

> 鉴权说明：
> - 只有 `auth.enabled: true` 且 `password` 非空时才会强制鉴权；缺少 `auth` 段或密码为空时 API 保持开放（删除 `auth` 段等价于关闭鉴权，请谨慎操作）。
> - 开启鉴权后，除 `/api/auth/*`（登录/状态/首次设置密码）外，所有 `/api/*` 接口都需要凭据：浏览器使用登录后下发的 httpOnly cookie（`ct_session`，SameSite=Strict，HTTPS 下带 Secure），并额外携带 `X-Requested-With: control-tower` 头用于防 CSRF；脚本/CLI 使用 `Authorization: Bearer <token>`。
> - 密码只能通过 Web UI 首次设置（`POST /api/auth/setup-password`），请求体是 PBKDF2 派生 key 而不是明文密码；首次设置只允许从本机（loopback）发起，已有密码时该接口会拒绝覆盖，重置请直接编辑 `settings.yaml`。
> - `settings.yaml` 中的 `auth.password` 是可直接用于登录的校验凭据，服务会以 `0600` 权限写入；请勿放宽该文件权限或把内容提交到版本库。
> - `GET /api/settings` 不会返回 `auth.password`；`GET /api/profiles` 不会返回 profile 的自定义请求头（可能含订阅 token）。
> - Web UI 只依赖 httpOnly cookie（登录响应里的 `token` 供脚本/CLI 使用，页面不会保存），不写入 `localStorage`；Sign Out 会吊销服务端 session 并让浏览器删除 cookie。

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

---

## 更新记录

### 0.3.0

- **系统代理**：Web UI 支持开启/关闭 Linux 桌面代理（GNOME/KDE 等），状态持久化到 `system-proxy.yaml`，服务重启或桌面会话恢复后自动重新应用；服务退出时自动还原，避免系统代理指向已关闭的端口。
- **策略组选择**：`/api/proxies/select` 支持指定策略组（默认 GLOBAL），选择结果持久化到 `proxy-selections.yaml`，切换订阅或重启后保留。
- **订阅流量与到期**：解析订阅响应头 `subscription-userinfo`，`ctctl profile list` 展示已用流量、总量、使用比例和到期日期。
- **分享链接导入**：Web UI 与 `/api/profiles/import-links` 支持一次粘贴多条 `vmess://`、`vless://`、`trojan://`、`ss://` 链接，生成本地 profile。
- **内核看护**：`ctsvc` 每 5 秒检查 Mihomo，意外退出后按指数退避自动重启；`service stop` 后不会被误拉起。
- **服务安装**：新增 `ctctl service install`，复制二进制与配置到 `/opt/ctsvc`、写入 systemd 单元并启用；`CT_USER` 可指定运行用户（默认 `SUDO_USER`）。
- **0.4 预览**：配置校验/备份/回滚（`/api/config/validate`、`/api/config/backups`）、规则匹配测试（`/api/rules/match`）、流量统计持久化与 `/api/stats`，Web UI 已提供对应入口。

### 0.2.4

- `ctctl profile update` 更新当前 profile 后自动替换 `config.yaml` 并重启服务，`ctctl proxy list` 立即反映新节点。
- `-H "User-Agent: ..."` 可覆盖 `profiles.yaml` 中已保存的 `user_agent`。

### 0.2.3

- 修复 `options.user_agent` 被 `options.headers` 中的 `user-agent` 覆盖的问题；优先级改为 `options.user_agent` > `headers.user-agent` > 默认值，并避免重复发送 UA。

### 0.2.2

- **profiles.yaml 并发安全**：所有读写改为 `update_profiles`（进程内 `Mutex` + 跨进程 `flock` + 锁内重读 + 原子写回），服务与 `ctctl` 同时修改不会再互相覆盖。
- **订阅下载体积上限**：服务端与 CLI 下载订阅时限制为 10 MiB，超限或非 UTF-8 内容直接报错，避免异常订阅耗尽内存。
- **登录失败限速**：同一来源 IP 在 60 秒内失败 5 次后返回 `429` 并带 `Retry-After`，成功登录后清零；限速只认 `peer_addr`，不信任 `X-Forwarded-For`。
- **Web UI 安全响应头**：新增 CSP、`X-Frame-Options: DENY`、`X-Content-Type-Options: nosniff`、`Referrer-Policy: no-referrer`。
- **依赖清理**：移除 `actix-files`、`actix-cors`、`clash_verge_service_ipc`、`compact_str`、`argon2`、`bcrypt`、`pbkdf2` 等未使用依赖；`zip` 仅 Windows 目标启用。

### 0.2.1

- `http_port` 改为映射 Mihomo 的 `port`（独立 HTTP 代理端口），不再复用 `mixed-port`；与 `mixed-port` 冲突时移除后者并记录告警。
- 鉴权覆盖所有 `/api/*`（全局中间件），Web UI 改用 httpOnly 会话 cookie + CSRF 头，脚本/CLI 继续使用 Bearer token。
- `settings.yaml`、`config.yaml`、profiles 与 IPC socket 以 `0600` 原子写入，配置目录 `0700`。
- 修复 `ctctl profile add/update -H` 丢失自定义请求头、未开启鉴权时误报 `401`、内置 CryptoJS 等问题。