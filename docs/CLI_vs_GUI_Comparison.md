# Control Tower vs 原版 Verge GUI 功能对比

## 概述

本文档对比 Control Tower CLI (`control-tower-cli`) 与原版 Clash Verge (Tauri GUI) 的功能差异。

---

## 功能对比表

### 服务管理

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 启动服务 | ✓ | `control-tower service start` | |
| 停止服务 | ✓ | `control-tower service stop` | |
| 重启服务 | ✓ | `control-tower service restart` | |
| 服务状态 | ✓ | `control-tower service status` | 显示 PID、运行时间 |
| 服务日志 | ✓ | `control-tower service logs` | 支持 --lines 参数 |

### 代理管理

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 代理列表 | ✓ | `control-tower proxy list` | CLI 显示索引号 |
| 选择代理 | ✓ | `control-tower proxy select` | 支持 #索引号、名称匹配 |
| 延迟测试 | ✓ | `control-tower proxy test` | 支持单个或全部代理 |
| 代理详情 | ✓ | ✗ | GUI 可查看详细节点信息 |

### 订阅管理

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 添加订阅 | ✓ | `control-tower profile add` | |
| 订阅列表 | ✓ | `control-tower profile list` | |
| 更新订阅 | ✓ | `control-tower profile update` | |
| 删除订阅 | ✓ | `control-tower profile remove` | |
| 定时更新 | ✓ | `control-tower profile cron` | 设置 cron 表达式 |
| 订阅编辑 | ✓ | ✗ | GUI 支持编辑订阅信息 |

### 模式管理

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 获取模式 | ✓ | `control-tower mode get` | |
| 设置模式 | ✓ | `control-tower mode set` | rule/global/direct |
| 规则编辑 | ✓ | ✗ | GUI 支持自定义规则 |

### 连接管理

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 连接列表 | ✓ | `control-tower connections` | CLI 显示流量统计 |
| 关闭连接 | ✓ | ✗ | |
| 连接详情 | ✓ | ✗ | GUI 显示完整连接信息 |

### 配置管理

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 查看配置 | ✓ | `control-tower config get` | |
| 修改配置 | ✓ | `control-tower config set` | |
| 配置文件 | ✓ | ✗ | GUI 支持编辑 config.yaml |

### 日志与统计

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 服务日志 | ✓ | `control-tower service logs` | |
| 流量统计 | ✓ | `control-tower connections` | 显示总上传/下载 |
| 节点统计 | ✓ | ✗ | GUI 显示各节点使用量 |

### Web UI

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| Web 管理界面 | ✗ | `control-tower web` | 启动嵌入式 Web UI |

### 系统集成

| 功能 | Verge GUI | CLI | 备注 |
|------|-----------|-----|------|
| 系统托盘 | ✓ | ✗ | |
| 开机启动 | ✓ | ✗ | |
| 全局代理 | ✓ | ✗ | |
| TUN 模式 | ✓ | ✗ | |

---

## CLI 独有功能

1. **脚本集成** - 可通过 shell 脚本自动化调用
2. **远程 SSH** - 可在远程机器上运行
3. **定时任务** - 可结合 cron 实现自动化
4. **轻量级** - 无需 GUI，适合服务器环境

---

## GUI 独有功能

1. **可视化规则编辑** - 图形化编辑代理规则
2. **连接详情** - 查看单个连接的详细信息
3. **系统托盘** - 后台运行，随时切换
4. **TUN 模式** - 处理所有流量
5. **全局代理** - 系统级代理设置

---

## 已完成的功能

### Phase 1
- [x] 修复编译警告
- [x] 实现 `proxy test` 延迟测试
- [x] 实现 `service start/restart`
- [x] 增强 help 文档

### Phase 2
- [x] 代理索引选择 (`#7`)
- [x] 代理部分匹配选择
- [x] 定时订阅更新 (`profile cron`)

### Phase 3
- [x] 服务日志 (`service logs`)
- [x] 流量统计显示
- [x] 连接列表增强

### Phase 4
- [x] 功能对比文档 (本文档)
- [x] 完整测试覆盖

---

## 待完成功能

- [ ] 连接管理功能增强：支持关闭连接
- [ ] 节点统计功能
- [ ] 全局代理和 TUN 模式支持
- [ ] 统一服务管理和部署功能
- [ ] Web UI 增强

---

## 使用建议

### 适合使用 CLI 的场景

- 服务器环境（无 GUI）
- 自动化脚本
- 远程 SSH 管理
- 快速测试代理延迟

### 适合使用 GUI 的场景

- 日常使用（系统托盘）
- 复杂的规则配置
- 需要 TUN 模式
- 可视化连接管理
