# Control Tower 使用指南

## 目录
1. [简介](#简介)
2. [安装](#安装)
3. [快速开始](#快速开始)
4. [命令参考](#命令参考)
5. [功能详解](#功能详解)
6. [常见问题](#常见问题)

---

## 简介

Control Tower 是一款命令行代理管理工具，支持：
- 多代理节点管理和切换
- 订阅配置文件管理
- 连接监控与管理
- 流量统计
- TUN 模式（VPN 模式）
- systemd 服务部署

---

## 安装

### 方式一：从源码编译

```bash
# 克隆仓库
git clone https://github.com/your-repo/control-tower.git
cd control-tower

# 编译
cargo build --release

# 安装二进制文件到 PATH
cp target/release/control-tower ~/.local/bin/
```

### 方式二：安装为系统服务

```bash
# 需要 root 权限
sudo control-tower service install

# 启动服务
sudo systemctl start control-tower

# 设置开机自启
sudo systemctl enable control-tower
```

---

## 快速开始

```bash
# 1. 启动服务
control-tower service start

# 2. 查看可用代理节点
control-tower proxy list

# 3. 选择代理节点（支持索引号、名称匹配）
control-tower proxy select "香港 101"
# 或使用索引
control-tower proxy select "#7"

# 4. 测试代理延迟
control-tower proxy test "香港 101"

# 5. 查看节点统计
control-tower proxy stats

# 6. 切换模式
control-tower mode set global    # 全局代理
control-tower mode set rule      # 规则模式
control-tower mode set direct     # 直连模式
```

---

## 命令参考

### 服务管理 (service)

```bash
control-tower service start      # 启动服务
control-tower service stop       # 停止服务
control-tower service restart   # 重启服务
control-tower service status     # 查看状态
control-tower service logs      # 查看日志
control-tower service logs --lines 100  # 查看更多日志
control-tower service install    # 安装为 systemd 服务
control-tower service uninstall  # 卸载 systemd 服务
```

### 代理管理 (proxy)

```bash
control-tower proxy list                  # 列出所有代理节点
control-tower proxy select "节点名称"     # 选择代理（支持部分匹配）
control-tower proxy select "#7"          # 按索引号选择
control-tower proxy test                  # 测试所有节点延迟
control-tower proxy test "节点名称"       # 测试指定节点
control-tower proxy stats                 # 显示节点统计信息
```

### 模式管理 (mode)

```bash
control-tower mode get                    # 获取当前模式
control-tower mode set rule               # 规则模式
control-tower mode set global             # 全局代理模式
control-tower mode set direct             # 直连模式
control-tower mode tun status            # 查看 TUN 状态
control-tower mode tun enable            # 启用 TUN 模式
control-tower mode tun disable           # 禁用 TUN 模式
```

### 连接管理 (connections)

```bash
control-tower connections list            # 列出所有连接
control-tower connections close <索引>   # 关闭指定连接
control-tower connections detail <索引>  # 查看连接详情
```

### 配置管理 (config)

```bash
control-tower config get                  # 获取当前配置
control-tower config set key=value       # 设置配置项
```

### 订阅管理 (profile)

```bash
control-tower profile add <URL>                    # 添加订阅
control-tower profile add <URL> "我的订阅"          # 添加命名订阅
control-tower profile list                          # 列出所有订阅
control-tower profile remove <ID>                   # 删除订阅
control-tower profile update                        # 更新当前订阅
control-tower profile update <ID>                   # 更新指定订阅
control-tower profile cron <ID> "0 6 * * *"       # 设置自动更新 cron
control-tower profile cron <ID> off                 # 禁用自动更新
```

### Web 界面

```bash
control-tower web                    # 启动 Web UI (默认 0.0.0.0:8080)
control-tower web --host 127.0.0.1 --port 9000  # 自定义地址
```

---

## 功能详解

### 1. 代理选择

**索引选择**
```bash
# 使用 # 前缀指定索引号
control-tower proxy select "#7"
```

**名称匹配**
```bash
# 精确匹配
control-tower proxy select "香港 101"

# 部分匹配（选择第一个匹配项）
control-tower proxy select "香港"

# 大小写不敏感
control-tower proxy select "hong kong"
```

### 2. TUN 模式

TUN 模式提供类似 VPN 的功能，系统所有流量都会经过代理。

```bash
# 查看状态
control-tower mode tun status

# 启用（需要重启服务生效）
control-tower mode tun enable
control-tower service restart

# 禁用
control-tower mode tun disable
control-tower service restart
```

**注意**: TUN 模式需要系统内核支持，通常在 Linux 上可用。

### 3. 节点统计

```bash
control-tower proxy stats
```

输出示例：
```
Proxy Statistics
======================================================================

 Index Name                           Type       Health     Connections
----------------------------------------------------------------------
   [1] DIRECT                         Direct     ● Healthy  0
   [2] REJECT                         Reject     ● Healthy  0
   [3] 香港 101 | 1x HK               Vless      ● Healthy  2
   [4] 香港 102 | 1x HK               Vless      ● Healthy  1
   ...

----------------------------------------------------------------------
Summary:
  Total proxies: 90
  Healthy: 88
  Dead: 2
  Active connections: 3
```

### 4. 连接管理

**查看连接**
```bash
control-tower connections list
```

输出示例：
```
Connections:
(Use web UI at http://127.0.0.1:8080 for full connections view)

Total Traffic:
  Upload:   1.76 KB
  Download: 459.04 KB

Active connections: 2
================================================================================
   ID. Destination          Type         Speed           Proxy
--------------------------------------------------------------------------------
   1.  example.com          HTTP         10.5 KB/s      香港 101
   2.  api.test.com         HTTPS        5.2 KB/s       香港 102
```

**关闭连接**
```bash
# 通过索引关闭
control-tower connections close 1
```

**查看详情**
```bash
# 查看连接的详细信息
control-tower connections detail 1
```

输出示例：
```
============================================================
Connection Details (Index #1)
============================================================
ID: abc123-def456

Meta Information:
  Destination: example.com
  Type: HTTP
  Host: example.com
  Process: /usr/bin/curl

Proxy: 香港 101

Traffic:
  Upload:     1.23 KB
  Download:   45.67 KB
  Speed:      10.5 KB/s

Chains (2 hops):
  1. 香港 101
  2. DIRECT

Rule: Match

Time: 2024-01-15 10:30:45
============================================================
```

### 5. 订阅管理

```bash
# 添加订阅
control-tower profile add https://example.com/sub.yaml

# 设置定时自动更新（每天早上6点）
control-tower profile cron <ID> "0 6 * * *"

# 每6小时更新一次
control-tower profile cron <ID> "0 */6 * * *"

# 禁用自动更新
control-tower profile cron <ID> off
```

### 6. 服务部署

```bash
# 安装为系统服务
sudo control-tower service install

# 服务将：
# - 在系统启动时自动运行
# - 由 systemd 管理
# - 在崩溃后自动重启

# 管理服务
sudo systemctl start control-tower
sudo systemctl stop control-tower
sudo systemctl restart control-tower
sudo systemctl status control-tower

# 卸载服务
sudo control-tower service uninstall
```

---

## 常见问题

### Q1: 服务启动失败 "Service socket not found"

**原因**: 服务未正常运行

**解决**:
```bash
# 检查服务状态
control-tower service status

# 如果未运行，启动服务
control-tower service start

# 如果启动失败，查看日志
control-tower service logs
```

### Q2: 代理选择失败 "No proxy found matching"

**原因**: 没有匹配到代理节点

**解决**:
```bash
# 先查看可用节点
control-tower proxy list

# 使用完整的节点名称
control-tower proxy select "香港 101"
```

### Q3: 连接关闭失败 "Connection not found"

**原因**: 指定的索引不存在或连接已关闭

**解决**:
```bash
# 先查看当前连接列表
control-tower connections list

# 使用正确的索引号
control-tower connections close <正确的索引>
```

### Q4: TUN 模式启用后无法联网

**原因**: TUN 模式需要服务重启才能生效，或系统不支持

**解决**:
```bash
# 1. 确保已执行重启
control-tower service restart

# 2. 检查 TUN 状态
control-tower mode tun status

# 3. 如果问题持续，可能是系统不支持 TUN
#    尝试禁用 TUN 模式
control-tower mode tun disable
control-tower service restart
```

### Q5: 订阅更新失败

**原因**: 网络问题或订阅 URL 无效

**解决**:
```bash
# 检查订阅 URL 是否可访问
curl -I <订阅URL>

# 手动更新订阅
control-tower profile update <订阅ID>

# 检查日志获取详细错误
control-tower service logs
```

### Q6: systemctl 服务启动失败

**原因**: 权限问题或服务配置错误

**解决**:
```bash
# 查看详细错误
sudo systemctl status control-tower

# 查看日志
sudo journalctl -u control-tower -n 50

# 重新安装服务
sudo control-tower service uninstall
sudo control-tower service install

# 启动服务
sudo systemctl start control-tower
```

### Q7: 无法连接到 Clash API

**原因**: Mihomo 服务未运行或端口被占用

**解决**:
```bash
# 检查服务状态
control-tower service status

# 检查端口是否被占用
lsof -i :9090

# 重启服务
control-tower service restart
```

### Q8: 安装 service install 失败 "Are you running as root?"

**原因**: 需要 root 权限才能写入 systemd 目录

**解决**:
```bash
# 使用 sudo
sudo control-tower service install
```

---

## 配置文件位置

| 类型 | 路径 |
|------|------|
| 配置文件 | `~/.config/control-tower/config.yaml` |
| 订阅配置 | `~/.config/control-tower/profiles.yaml` |
| Mihomo 二进制 | `~/.config/control-tower/mihomo` |
| 服务 Socket | `~/.config/control-tower/control-tower-service.sock` |
| systemd 服务 | `/etc/systemd/system/control-tower.service` |

---

## 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `RUST_LOG` | 日志级别 | `info` |

```bash
# 设置调试日志
RUST_LOG=debug control-tower service start
```

---

## 获取帮助

```bash
# 查看主帮助
control-tower --help

# 查看子命令帮助
control-tower service --help
control-tower proxy --help
control-tower mode --help
control-tower connections --help
```
