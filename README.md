# Control Tower / 塔台

A lightweight proxy management tool built on [Mihomo](https://github.com/MetaCubeX/mihomo)，提供 CLI 命令行和 Web UI 两种交互方式，支持订阅自动更新、节点延迟测试、流量统计等进阶功能。

A lightweight management tool for Mihomo ( Clash.Meta ) , featuring both CLI and Web UI, with auto-updating subscriptions, proxy latency testing, traffic statistics, and more.

---

## 特性 / Features

- **Web UI** — 嵌入式 Web 管理界面，无需额外依赖，开箱即用
- **订阅管理 / Subscription Management** — 支持多订阅源，可配置定时自动更新（直连或走代理），手动一键刷新
- **节点测速 / Proxy Latency Testing** — 一键批量检测节点延迟
- **流量统计 / Traffic Statistics** — 实时展示当前速率与累计流量
- **规则管理 / Rule Management** — 可视化添加、删除、导入导出规则

---

## 快速开始 / Quick Start

### 环境要求 / Requirements

- Linux（amd64 或 aarch64）
- 已安装 [Mihomo](https://github.com/MetaCubeX/mihomo)（由脚本自动下载）
- systemd（用于服务模式）

### 构建 / Build

```bash
./build.sh
```

产物位于 `target/release/`：

- `ctsvc` — 后台服务守护进程
- `ctctl` — CLI 工具

### 一键安装服务 / Install as Service

```bash
cd target/dist/control-tower
sudo ./install-service.sh
```

服务将部署至 `/opt/ctsvc`，并注册为 systemd 单元 `ctsvc`。

### 启动与停止 / Start & Stop

```bash
# 通过 systemd
sudo systemctl start ctsvc   # 启动
sudo systemctl stop ctsvc    # 停止
sudo systemctl status ctsvc  # 状态
```

### Web UI

安装服务后，浏览器打开 **http://127.0.0.1:8080**

不安装服务时，也可直接启动内置 Web 服务器：

```bash
./ctctl web
# 浏览器打开 http://127.0.0.1:8080
```

---

## CLI 命令参考 / CLI Reference

```bash
# 节点管理 / Proxy Management
ctctl proxy list         # 列出所有节点
ctctl proxy select <名称> # 选择节点
ctctl proxy test        # 测速所有节点

# 模式切换 / Mode Management
ctctl mode get          # 查看当前模式
ctctl mode set global   # 切换为全局模式
ctctl mode set rule     # 切换为规则模式
ctctl mode set direct   # 切换为直连模式

# 规则管理 / Rule Management
ctctl rule list         # 查看当前规则
ctctl rule add <规则>   # 添加规则
ctctl rule remove <序号> # 删除规则
ctctl rule import <文件> # 导入规则文件
ctctl rule export       # 导出规则

# 订阅管理 / Profile Management
ctctl profile list                  # 列出所有订阅
ctctl profile add <URL> [名称]       # 添加订阅
ctctl profile remove <ID>           # 删除订阅
ctctl profile activate <ID>        # 激活订阅
ctctl profile cron <ID> <表达式>    # 设置自动更新 cron 表达式（分钟粒度）
ctctl profile update                # 手动更新当前订阅

# 连接管理 / Connection Management
ctctl connections list    # 查看当前连接
ctctl connections close <ID> # 关闭指定连接

# 帮助 / Help
ctctl --help
```

---

## Web UI 页面说明 / Web UI Pages

| 页面 / Page | 说明 / Description |
|-------------|-------------------|
| **Dashboard** | 系统概览：节点数、连接数、模式、当前节点、上下行速率、累计流量；5 秒自动刷新 |
| **Proxies** | 节点列表，支持搜索、分页、延迟测试，支持一键切换节点 |
| **Profiles** | 订阅管理：添加/删除/激活订阅，查看拉取时间，支持手动刷新（直连 ↻D / 代理 ↻P） |
| **Rules** | 可视化规则管理：查看、添加、删除规则 |
| **Connections** | 实时连接列表：来源/目标/节点/流量/时长，支持一键断开 |
| **Settings** | Mihomo 端口配置（HTTP / SOCKS5 / API），日志查看器（ctsvc / mihomo），服务启停 |

## settings.yaml 配置项 / Settings

```yaml
api_host: 127.0.0.1      # Mihomo API 监听地址
api_port: 9090           # Mihomo API 端口
http_port: 7890          # HTTP 代理端口
socks_port: 7891         # SOCKS5 代理端口
service_port: 8080       # Web UI 监听端口
tun_enabled: false       # 是否启用 TUN 模式
log_level: info          # 日志级别
mode: rule               # 默认代理模式
```
---

## 架构说明 / Architecture

```
┌─────────────────────────────────────────────┐
│              Web Browser                     │
│         http://127.0.0.1:8080                │
└───────────────┬─────────────────────────────┘
                │ HTTP
┌───────────────▼─────────────────────────────┐
│           ctsvc (HTTP Server)               │
│    actix-web · /api/* · embedded HTML      │
└────────┬──────────────────────┬────────────┘
         │ IPC (Unix Socket)     │ HTTP
┌────────▼──────┐    ┌─────────▼────────────┐
│  ctctl (CLI)  │    │    Mihomo (Subprocess) │
│  User CLI     │    │  clash-verge-service   │
│  Interactive  │    │  Port 9090 API         │
└───────────────┘    └───────────────────────┘
```

- **ctsvc** — 无头 HTTP 服务器，提供 API 与 Web UI；以 systemd 守护进程运行
- **ctctl** — 用户 CLI 工具，通过 Unix Socket 与 ctsvc 通信
- **Mihomo** — 由 ctsvc 管理的子进程，提供真正的代理能力

