# ctctl 场景使用指南

> 适用版本：Control Tower 0.3.1（Linux）
> 面向已经安装 `ctsvc` 的用户，按“要完成什么事”组织，而不是按命令字母排序。
> Web UI：`http://127.0.0.1:8080`（启用 HTTPS 后为 `https://`）

---

## 0. 先分清三个东西

| 组件 | 作用 |
|---|---|
| `ctsvc` | 常驻守护进程。负责启动/看护 Mihomo、加载配置、定时更新订阅、系统代理、统计落盘，并提供 Web UI / HTTP API / IPC。 |
| Mihomo | 真正的代理内核，由 `ctsvc` 作为子进程管理。 |
| `ctctl` | 命令行客户端。常用运维走 IPC 交给 `ctsvc`；订阅、规则等操作会直接读写配置目录中的文件。 |

### 0.1 安装后的文件位置

默认安装目录是 `/opt/ctsvc`：

| 路径 | 内容 |
|---|---|
| `/opt/ctsvc/ctsvc` | 守护进程 |
| `/opt/ctsvc/ctctl` | CLI |
| `/opt/ctsvc/settings.yaml` | 全局设置（`0600`，可能含鉴权凭据） |
| `/opt/ctsvc/config.yaml` | 当前生效的 Mihomo 配置 |
| `/opt/ctsvc/profiles.yaml` | 订阅清单 |
| `/opt/ctsvc/profiles/*.yaml` | 各订阅下载下来的配置 |
| `/opt/ctsvc/backups/` | Web UI 生成的配置备份（最多 20 份） |
| `/opt/ctsvc/logs/ctsvc.log*` | 服务日志（按天轮转） |
| `/opt/ctsvc/logs/mihomo.log` | Mihomo 日志 |
| `/tmp/ctsvc.sock` | `ctctl` 与 `ctsvc` 的 IPC socket（`0600`） |
| `/opt/ctsvc/ctsvc.pid` | 守护进程 PID |

默认端口：

| 端口 | 用途 |
|---|---|
| `8080` | Web UI / HTTP API |
| `9090` | Mihomo 外部控制器 API（仅监听 `127.0.0.1`） |
| `7890` | HTTP 代理 |
| `7891` | SOCKS5 代理 |

### 0.2 两种“重启”不是一回事

| 操作 | 影响范围 |
|---|---|
| `sudo systemctl restart ctsvc` | 重启守护进程，连带重启 Mihomo |
| `sudo /opt/ctsvc/ctctl service restart` | 只重启 Mihomo，守护进程保持运行 |
| `ctctl service stop` | 停止 Mihomo，守护进程仍在；内核看护不会把它拉起来 |
| `ctctl service start` | 启动 Mihomo；若守护进程不存在则尝试拉起 `ctsvc` |

只改代理配置、切换订阅时用 `ctctl service restart` 就够了；升级二进制、改 `settings.yaml` 时用 `systemctl restart ctsvc`。

### 0.3 运行身份与配置查找

- IPC socket 和配置文件都是 `0600`，`ctctl` 通常要以服务运行用户或 root 执行。
- 安装脚本默认把服务运行用户设为执行 `sudo` 的用户（`SUDO_USER`），可用 `CT_USER` 覆盖。
- 如果服务以 `alice` 运行，推荐：

  ```bash
  sudo -u alice /opt/ctsvc/ctctl service status
  ```

  或者把 `/opt/ctsvc` 加入 `PATH` 后直接 `sudo ctctl ...`。
- `ctctl` 查找设置文件的顺序：
  1. `--config <路径>`（必须放在子命令前：`ctctl --config ... profile list`）
  2. `ctctl` 可执行文件同目录的 `settings.yaml`
  3. `~/.config/control-tower/settings.yaml`

  因此请始终运行安装目录里的 `ctctl`，不要从任意目录运行副本，否则可能在错误位置创建 `settings.yaml`。

---

## 1. 安装、升级与验证

### 1.1 首次安装

```bash
tar -xzf control-tower.tar.gz
cd control-tower
sha256sum -c SHA256SUMS
sudo ./install-service.sh
# 或者（升级时更推荐，会保留已有配置和数据）
sudo ./ctctl service install
```

指定服务运行用户：

```bash
CT_USER=alice sudo -E ./ctctl service install
```

安装完成后：

```bash
sudo systemctl status ctsvc
sudo /opt/ctsvc/ctctl service status
```

浏览器打开 `http://127.0.0.1:8080`，首次访问按提示设置 Web UI 密码（只允许从本机 loopback 设置）。

### 1.2 升级

```bash
tar -xzf control-tower-新版本.tar.gz
cd control-tower
sudo ./ctctl service install
```

`ctctl service install` 只替换二进制和 Mihomo，不会覆盖已有的 `settings.yaml`、`profiles.yaml`、`profiles/` 和 `config.yaml`。

> 升级时不要使用 `install-service.sh`：它会用包内的默认 `settings.yaml` 覆盖 `/opt/ctsvc/settings.yaml`，可能丢失端口、鉴权和系统代理等设置。

升级前建议在 Web UI 的 Settings → Config 里点一次 **Backup now**，并额外备份：

```bash
sudo cp -a /opt/ctsvc/settings.yaml /opt/ctsvc/profiles.yaml /root/ct-backup-$(date +%F)/
```

### 1.3 验证

```bash
sudo /opt/ctsvc/ctctl service status     # Status: Running 即内核正常
curl -s http://127.0.0.1:8080/api/status # 未开鉴权时可直接访问
ss -ltnp | grep -E ':(8080|7890|7891|9090)\b'
```

### 1.4 卸载

```bash
cd control-tower
sudo ./uninstall-service.sh
```

> 卸载脚本会删除整个 `/opt/ctsvc`，包括 `settings.yaml`、`profiles.yaml`、订阅文件和配置备份。卸载前请自行备份。

### 1.5 忘记 Web UI 密码

密码只保存为 `settings.yaml` 里的校验值，无法反推。可在服务器上：

```bash
sudo nano /opt/ctsvc/settings.yaml
```

删除 `auth` 段（或把 `auth.enabled` 改为 `false`）后：

```bash
sudo systemctl restart ctsvc
```

再次访问 Web UI 即可重新设置密码。操作前先备份 `settings.yaml`。

---

## 2. 订阅管理

### 2.1 添加订阅

```bash
sudo /opt/ctsvc/ctctl profile add 'https://example.com/sub.yaml' 'My Sub'
```

需要自定义请求头（可重复）：

```bash
sudo /opt/ctsvc/ctctl profile add 'https://example.com/sub.yaml' 'My Sub' \
  -H 'User-Agent: clash-verge/v2.5.10' \
  -H 'Authorization: Bearer xxxxx'
```

说明：

- `profile add` 只下载并登记，**不会自动激活**，需要再执行 `profile activate`。
- 请求头会保存在 `profiles.yaml` 的 `options.headers` 中，后续更新自动沿用。
- 部分机场按 `User-Agent` 判断客户端版本。若下载到的是“您的客户端已过时”之类的提示节点，改用：

  ```bash
  sudo /opt/ctsvc/ctctl profile update <ID> -H 'User-Agent: clash-verge/v2.5.10'
  ```

### 2.2 查看订阅与流量

```bash
sudo /opt/ctsvc/ctctl profile list
```

输出包含 ID、名称、是否激活、最后更新时间、自动更新间隔；如果订阅响应带 `subscription-userinfo`，还会显示：

```text
  traffic: 12.34 GiB / 100.00 GiB (12.3%)
  expires: 2027-01-01
```

### 2.3 激活 / 切换订阅

```bash
sudo /opt/ctsvc/ctctl profile list
sudo /opt/ctsvc/ctctl profile activate <ID>
```

激活会把该订阅的配置文件复制为 `config.yaml` 并重启 Mihomo。

> 注意：激活会**整体替换** `config.yaml`，手工加到 `config.yaml` 里的规则会丢失（见 5.4）。

### 2.4 更新订阅

更新当前激活订阅（更新后会立即替换 `config.yaml` 并重启 Mihomo）：

```bash
sudo /opt/ctsvc/ctctl profile update
```

更新指定订阅：

```bash
sudo /opt/ctsvc/ctctl profile update <ID>
```

输出会打印本次实际使用的 User-Agent，便于核对：

```text
Updating profile-xxx from https://... (User-Agent: clash-verge/v2.5.10)
Active config updated: /opt/ctsvc/config.yaml
Restarting service to apply the refreshed config...
```

如果更新的是非激活订阅，只刷新 profile 文件，不影响正在运行的代理。

### 2.5 自动更新

单位是**分钟**，范围 1–10080：

```bash
sudo /opt/ctsvc/ctctl profile cron <ID> 60    # 每 60 分钟
sudo /opt/ctsvc/ctctl profile cron <ID>       # 查看当前设置
sudo /opt/ctsvc/ctctl profile cron <ID> off   # 关闭
```

`ctsvc` 每 60 秒检查一次到期任务；修改后会立即通过 IPC 重新加载任务列表。

### 2.6 删除订阅

```bash
sudo /opt/ctsvc/ctctl profile list
sudo /opt/ctsvc/ctctl profile remove <ID>
```

0.3.1 起，只有删除**当前激活**的订阅才会停止内核并删除 `config.yaml`；删除其它订阅不会再影响正在使用的代理。

### 2.7 分享链接导入

把 vmess/vless/trojan/ss/hysteria2 分享链接导入为本地订阅（0.3.1 新增）：

```bash
sudo /opt/ctsvc/ctctl profile import 'vmess://...' 'vless://...'
sudo /opt/ctsvc/ctctl profile import --file /tmp/links.txt --name 'My nodes' --activate
```

说明：

- `--file` 与位置参数可同时使用，逐行解析；单条格式错误会打印警告并跳过，不影响其它节点。
- 导入后生成一个 `type: local` 的 profile；不带 `--activate` 时需要手动 `profile activate <ID>`。
- 导入内容会保存在 `profiles/<ID 前 8 位>.yaml`，权限 `0600`。

### 2.8 确认下载内容是否正常

不要只看 `ctctl proxy list`，它读取的是正在运行的 Mihomo。先检查刚下载的文件：

```bash
head -40 /opt/ctsvc/profiles/<profile-id>.yaml
```

- 是正常 `proxies:` 节点 → 执行 `activate` 或 `update` 让其生效。
- 仍是“客户端已过时 / 请访问我们的网站 / 下载新版客户端” → 订阅源拒绝了当前请求，优先换 UA 或用 `-H` 覆盖，再检查是否需要其他请求头。

---

## 3. 节点与策略组

### 3.1 查看与选择节点

```bash
sudo /opt/ctsvc/ctctl proxy list
sudo /opt/ctsvc/ctctl proxy select '#7'          # 按列表索引（1 开始）
sudo /opt/ctsvc/ctctl proxy select '香港 101'     # 精确名称
sudo /opt/ctsvc/ctctl proxy select '香港'         # 模糊匹配，多个时选第一个
```

名称含空格或中文时请加引号。CLI 的选择结果还会写入 `settings.yaml` 的 `selected_proxy`。

### 3.2 测速与状态

```bash
sudo /opt/ctsvc/ctctl proxy test                 # 测试 GLOBAL 组全部节点
sudo /opt/ctsvc/ctctl proxy test '香港 101'       # 测试单个节点
sudo /opt/ctsvc/ctctl proxy stats                # 延迟 / 健康状态 / 连接数
```

测速使用 `http://cp.cloudflare.com/generate_204`。

### 3.3 策略组

0.3.1 起 CLI 可以按策略组查看和选择节点（例如“自动选择”“流媒体”“AI”）：

```bash
sudo /opt/ctsvc/ctctl proxy list --group '节点选择'
sudo /opt/ctsvc/ctctl proxy select --group '节点选择' '香港 101'
sudo /opt/ctsvc/ctctl proxy select --group '节点选择' '#3'
```

不带 `--group` 时仍只操作 **GLOBAL** 组。选择结果会由守护进程持久化到：

```text
/opt/ctsvc/proxy-selections.yaml
```

切换订阅或重启后仍会尝试恢复。若 IPC 不可用，会退化为直接写 Mihomo API，但不会写入该文件。

### 3.4 节点全部超时的排查顺序

1. `ctctl service status` 是否 Running；
2. `ctctl proxy list` 是否有真实节点（不是机场提示节点）；
3. 当前模式是否为 `direct`：`ctctl mode get`；
4. 是否选中了不可用节点：换一个节点再 `proxy test`；
5. `mihomo.log` 是否有 DNS / TLS / 连接失败；
6. 若启用 TUN，确认有 `CAP_NET_ADMIN` 且没有与其他 VPN 冲突。

---

## 4. 模式与 TUN

### 4.1 规则 / 全局 / 直连

```bash
sudo /opt/ctsvc/ctctl mode get
sudo /opt/ctsvc/ctctl mode set rule
sudo /opt/ctsvc/ctctl mode set global
sudo /opt/ctsvc/ctctl mode set direct
```

### 4.2 TUN

```bash
sudo /opt/ctsvc/ctctl mode tun status
sudo /opt/ctsvc/ctctl mode tun enable
sudo /opt/ctsvc/ctctl mode tun disable
```

TUN 变更会重启 Mihomo，需要 root 或 `CAP_NET_ADMIN`（systemd 单元已配置）。仅 Linux 支持。

---

## 5. 规则管理

### 5.1 查看、添加、删除

```bash
sudo /opt/ctsvc/ctctl rule list
sudo /opt/ctsvc/ctctl rule list --limit 20
sudo /opt/ctsvc/ctctl rule add 'DOMAIN-SUFFIX,google.com,香港 101'
sudo /opt/ctsvc/ctctl rule add 'GEOIP,CN,DIRECT'
sudo /opt/ctsvc/ctctl rule remove 5
```

索引是 `rule list` 显示的 1-based 序号。

### 5.2 导入 / 导出

```bash
sudo /opt/ctsvc/ctctl rule export /tmp/rules.yaml
sudo /opt/ctsvc/ctctl rule import /tmp/rules.yaml
```

支持纯文本（每行一条）或带 `rules:` 键的 YAML。

### 5.3 让规则生效

`rule add/remove/import` 只修改 `config.yaml`，不会热加载。执行：

```bash
sudo /opt/ctsvc/ctctl service restart
```

### 5.4 规则持久化警告

`ctctl rule` 和 Web UI 的 Rules 页面修改的是**当前生效配置** `config.yaml`。以下操作会整体替换 `config.yaml`，手工规则可能丢失：

- `ctctl profile activate <ID>`
- `ctctl profile update`（更新的是当前激活订阅时）
- Web UI 中刷新当前订阅

建议：

1. 改规则前先 `rule export` 备份；
2. 更新/切换订阅后重新 `rule import`；
3. 需要长期保留的规则，应写进订阅源或使用 merge 配置（当前版本尚未在 UI/CLI 暴露 merge 编辑）。

### 5.5 规则匹配测试

```bash
sudo /opt/ctsvc/ctctl rule match google.com
sudo /opt/ctsvc/ctctl rule match 'https://example.com:8443/path' --json
```

0.3.1 起 CLI 可以直接读取 `config.yaml` 并用与 Web UI 相同的规则引擎做离线匹配：输出每条规则是命中、跳过还是未命中，以及最终策略。`RULE-SET`、`GEOIP`、`GEOSITE` 等需要外部数据或运行内核的规则会被标记为 `SKIP`，不会假装未命中。服务离线时也能排障。

---

## 6. 连接查看

```bash
sudo /opt/ctsvc/ctctl connections list
sudo /opt/ctsvc/ctctl connections detail 1
sudo /opt/ctsvc/ctctl connections close 1
sudo /opt/ctsvc/ctctl connections top            # 实时监控，默认 1 秒刷新
sudo /opt/ctsvc/ctctl connections top -i 2
```

`connections top` 运行时按回车输入索引可直接关闭连接，`Ctrl+C` 退出。

连接历史（已关闭连接）持久化在：

```text
/opt/ctsvc/connection_history.yaml
```

0.3.1 起可直接查看和清空：

```bash
sudo /opt/ctsvc/ctctl connections history
sudo /opt/ctsvc/ctctl connections history -n 20
sudo /opt/ctsvc/ctctl connections history --json
sudo /opt/ctsvc/ctctl connections history --clear
```

该命令直接读文件，服务离线也能查看。

### 6.1 流量统计（0.3.1 新增）

```bash
sudo /opt/ctsvc/ctctl stats              # 累计 / 今日 / 近 7 天 / Top 节点与域名
sudo /opt/ctsvc/ctctl stats --json
sudo /opt/ctsvc/ctctl stats --reset      # 清空统计（同时清掉服务端内存缓存）
```

统计数据由 `ctsvc` 在连接关闭时累积并每 10 秒落盘，保存在 `/opt/ctsvc/stats.yaml`。`--reset` 必须经 IPC 调用守护进程，否则仅删文件会被内存缓存重新写回。

### 6.2 系统代理（0.3.1 新增）

桌面环境（GNOME/KDE）可由 CLI 开关系统代理：

```bash
sudo /opt/ctsvc/ctctl system-proxy status
sudo /opt/ctsvc/ctctl system-proxy on
sudo /opt/ctsvc/ctctl system-proxy off
```

该命令通过 IPC 交给守护进程执行，因此写入的是服务运行用户的桌面会话设置；`http_port` / `socks_port` 取自 `settings.yaml`。无桌面会话（纯服务器）时会报告 `unsupported`。

### 6.3 Rule Providers（0.3.1 新增）

```bash
sudo /opt/ctsvc/ctctl rule-provider list
sudo /opt/ctsvc/ctctl rule-provider list --json
sudo /opt/ctsvc/ctctl rule-provider refresh <名称>
sudo /opt/ctsvc/ctctl rule-provider refresh --all
```

列表来自 `settings.yaml` 的 `rule-providers`，并用 `config.yaml` 的状态补全；`refresh <名称>` 调 Mihomo 的 `/providers/rules/{name}`，`--all` 走强制重载配置的路径（与 Web UI 一致）。节点/规则未生效时先刷新 provider，再 `ctctl service restart`。

### 6.4 settings.yaml 通用读写（0.3.1 新增）

```bash
sudo /opt/ctsvc/ctctl settings get                 # 打印完整 settings.yaml
sudo /opt/ctsvc/ctctl settings get api_port
sudo /opt/ctsvc/ctctl settings set api_port 9090
sudo /opt/ctsvc/ctctl settings set mode rule
```

值按 YAML 解析（数字/布尔/字符串），写回时仍保持 `0600`。修改 `api_port`、`http_port`、`socks_port`、`service_port`、`tun_enabled`、`https` 后必须重启守护进程：

```bash
sudo systemctl restart ctsvc
```

> 注意：`settings.yaml` 可能包含 Web UI 鉴权校验值，`settings get` 不带 key 会原样打印，请勿把输出贴到公开渠道。

---

## 7. 排障场景

### 7.1 服务起不来

```bash
sudo systemctl status ctsvc
sudo journalctl -u ctsvc -n 100 --no-pager
sudo tail -n 100 /opt/ctsvc/logs/ctsvc.log*
sudo tail -n 100 /opt/ctsvc/logs/mihomo.log
```

检查项：

- 配置是否合法：

  ```bash
  /opt/ctsvc/mihomo/mihomo -t -f /opt/ctsvc/config.yaml
  ```

- 端口是否被占用：

  ```bash
  ss -ltnp | grep -E ':(8080|7890|7891|9090)\b'
  ```

- `/opt/ctsvc` 与 `/tmp/ctsvc.sock` 的属主/权限；
- 是否有残留 socket 但进程已退出：`sudo rm -f /tmp/ctsvc.sock` 后重启。

### 7.2 Web UI 打不开

```bash
sudo systemctl status ctsvc
ss -ltnp | grep ':8080'
sudo grep -E 'service_port|https' /opt/ctsvc/settings.yaml
```

- 启用了 HTTPS 就必须用 `https://`；
- 只监听 loopback 时不能从其他机器直接访问，请用 SSH 端口转发；
- 开启鉴权后需要先登录。

### 7.3 服务显示 Running，但浏览器没网

按顺序检查：

```bash
sudo /opt/ctsvc/ctctl service status
sudo /opt/ctsvc/ctctl mode get
sudo /opt/ctsvc/ctctl proxy list
sudo /opt/ctsvc/ctctl proxy test
sudo tail -n 100 /opt/ctsvc/logs/mihomo.log
```

常见原因：

- 模式是 `direct`；
- 选中的节点已失效；
- 系统代理没开（桌面环境）或指向了错误端口；
- TUN 与其它 VPN / 防火墙冲突；
- DNS 配置不可用。

### 7.4 订阅更新失败

```bash
sudo /opt/ctsvc/ctctl profile update <ID> -H 'User-Agent: clash-verge/v2.5.10'
head -40 /opt/ctsvc/profiles/<ID>.yaml
```

检查：

- 输出里的 `User-Agent` 是否为订阅源要求的版本；
- 是否需要 `Authorization` / `X-Token` 等请求头；
- 机场是否限制单 IP、过期或流量耗尽（`profile list` 可见）；
- 是否返回了 HTML 拦截页；
- 文件是否超过 10 MiB 上限；
- 是否需要走代理下载：在 `profiles.yaml` 对应条目加入 `options.with_proxy: true` 后重试。

`ctctl profile update` 会打印实际 UA；文件内容正常但节点列表仍旧，说明只是尚未应用，重新 `activate` 当前订阅即可。

### 7.5 配置改坏了怎么回滚

0.3.1 起 CLI 与 Web UI 共用同一套校验、备份和回滚逻辑：

```bash
sudo /opt/ctsvc/ctctl config show                     # 查看当前 config.yaml
sudo /opt/ctsvc/ctctl config validate                 # 只校验，不保存
sudo /opt/ctsvc/ctctl config backup list              # 列出备份（最新在前）
sudo /opt/ctsvc/ctctl config backup create -r manual  # 手动备份
sudo /opt/ctsvc/ctctl config backup restore <ID>      # 校验 → 备份 → 热加载 → 失败回滚
sudo /opt/ctsvc/ctctl config backup delete <ID>
sudo /opt/ctsvc/ctctl config edit                     # $EDITOR 编辑，保存时走同一安全流程
```

`config edit` 使用 `$VISUAL` / `$EDITOR`（默认 `vi`）打开临时副本；改完会先校验，确认后才备份、写入并让 Mihomo 热加载。Mihomo 拒绝新配置时会自动恢复旧配置，不会把代理弄挂。

`config show / validate` 和 `config backup list/create/delete` 在本地读文件，服务离线也可用；`config edit / backup restore` 需要守护进程在线。备份目录仍是 `/opt/ctsvc/backups/`，与 Web UI 完全兼容，最多保留 20 份。

### 7.6 权限类错误

典型报错：`Permission denied`、`Failed to connect to IPC socket`。

- 确认执行用户与服务运行用户一致（见 0.3 节）；
- 检查：

  ```bash
  ls -l /tmp/ctsvc.sock
  sudo ls -ld /opt/ctsvc /opt/ctsvc/*.yaml /opt/ctsvc/profiles
  ```

- 不要用 `chmod 777` 绕过；应调整属主或用正确的用户执行。

### 7.7 日志在哪里

| 日志 | 路径 |
|---|---|
| 服务运行日志 | `/opt/ctsvc/logs/ctsvc.log`（当前）与 `ctsvc.log.YYYY-MM-DD` |
| Mihomo 日志 | `/opt/ctsvc/logs/mihomo.log` |
| systemd 标准输出 | `journalctl -u ctsvc` |
| Web UI | Logs 页面，可切换服务 / Mihomo |

0.3.1 起可直接用 CLI（直接读文件，服务离线也能用）：

```bash
sudo /opt/ctsvc/ctctl logs                       # 服务日志最后 100 行
sudo /opt/ctsvc/ctctl logs -n 200 --source mihomo
sudo /opt/ctsvc/ctctl logs -n 50 -f              # 跟随（Ctrl+C 退出，自动跟随日志轮转）
```

`--source service` 会挑最新的 `ctsvc.log*`（含按天轮转的文件）。

---

## 8. 自动化示例

### 8.1 定时更新所有订阅

```bash
#!/bin/sh
set -eu
CT=/opt/ctsvc/ctctl
"$CT" profile list | awk '/^profile-/ {print $1}' | while read -r id; do
  echo "== updating $id"
  "$CT" profile update "$id" || echo "update failed: $id" >&2
done
```

> 只有当前激活订阅的更新会自动应用到 `config.yaml` 并重启 Mihomo；其它订阅只刷新文件。

配合 cron：

```cron
30 6 * * * root /usr/local/bin/update-profiles.sh >> /var/log/ct-update.log 2>&1
```

### 8.2 健康检查

```bash
#!/bin/sh
set -eu
CT=/opt/ctsvc/ctctl
"$CT" service status | grep -q 'Status: Running'
"$CT" proxy test '香港 101' >/dev/null
```

或者直接探测 Mihomo API：

```bash
curl -fsS http://127.0.0.1:9090/version >/dev/null
```

### 8.3 升级脚本

```bash
#!/bin/sh
set -eu
cd /tmp
curl -fLO https://github.com/JasonJe/control-tower/releases/latest/download/control-tower.tar.gz
curl -fLO https://github.com/JasonJe/control-tower/releases/latest/download/SHA256SUMS
sha256sum -c SHA256SUMS
rm -rf control-tower && tar -xzf control-tower.tar.gz
cd control-tower
sudo ./ctctl service install
```

---

## 9. 命令速查

| 场景 | 命令 |
|---|---|
| 服务状态 | `ctctl service status` |
| 启动/停止内核 | `ctctl service start` / `stop` / `restart` |
| 安装 systemd | `sudo ctctl service install` |
| 订阅列表/流量 | `ctctl profile list` |
| 添加订阅 | `ctctl profile add <URL> [名称] [-H 'K: V']` |
| 分享链接导入 | `ctctl profile import <link...> [--file F] [--name N] [--activate]` |
| 激活订阅 | `ctctl profile activate <ID>` |
| 更新订阅 | `ctctl profile update [ID] [-H 'K: V']` |
| 自动更新 | `ctctl profile cron <ID> <分钟|off>` |
| 节点列表 | `ctctl proxy list [--group <组名>]` |
| 选择节点 | `ctctl proxy select '#索引'` 或名称，`--group <组名>` 指定策略组 |
| 测速 | `ctctl proxy test [名称]` |
| 节点状态 | `ctctl proxy stats` |
| 模式 | `ctctl mode get` / `set rule|global|direct` |
| TUN | `ctctl mode tun status|enable|disable` |
| 规则列表 | `ctctl rule list [--limit N]` |
| 增删规则 | `ctctl rule add 'TYPE,VALUE,PROXY'` / `rule remove <序号>` |
| 规则导入导出 | `ctctl rule import <文件>` / `rule export [文件]` |
| 规则匹配测试 | `ctctl rule match <域名|IP|URL> [--json]` |
| 配置查看/校验 | `ctctl config show [--json]` / `config validate [文件]` |
| 配置备份 | `ctctl config backup list|create|restore <ID>|delete <ID>` |
| 配置编辑 | `ctctl config edit` |
| 连接 | `ctctl connections list|detail <序号>|close <序号>|top|history` |
| 日志 | `ctctl logs [-n N] [--source service|mihomo] [-f]` |
| 流量统计 | `ctctl stats [--json] [--reset]` |
| 系统代理 | `ctctl system-proxy status|on|off` |
| Rule Providers | `ctctl rule-provider list|refresh <名称|--all>` |
| 设置读写 | `ctctl settings get [key]` / `settings set <key> <value>` |
| 更新内核/数据 | `ctctl update all|mihomo|geoip|geosite` |
| 指定配置 | `ctctl --config /opt/ctsvc/settings.yaml ...` |

---

## 10. 0.3.1 CLI 能力边界

0.3.1 已把 P0/P1 全部补齐到 CLI，与 Web UI 的差距只剩以下几项（均不建议在 CLI 复刻）：

| 能力 | 推荐方式 |
|---|---|
| 登录 / 改 Web UI 密码 / 会话管理 | Web UI；CLI 不携带 HTTP token，继续用 `0600` IPC socket |
| 仪表盘实时速率图 | Web UI |
| 订阅 merge / script 等高级编辑 | Web UI 或直接编辑 `profiles.yaml` |
| 修改鉴权相关设置后立即生效 | Web UI 或 `ctctl settings set` 后 `systemctl restart ctsvc` |

0.3.1 新增命令一览：`config show|validate|backup|edit`、`logs`、`rule match`、`proxy list/select --group`、`profile import`、`stats`、`connections history`、`system-proxy`、`rule-provider`、`settings`。
