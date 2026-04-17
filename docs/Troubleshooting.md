# Control Tower 故障排查手册

## 目录
1. [服务问题](#服务问题)
2. [连接问题](#连接问题)
3. [代理问题](#代理问题)
4. [配置问题](#配置问题)
5. [网络问题](#网络问题)
6. [TUN 模式问题](#tun-模式问题)
7. [安装问题](#安装问题)

---

## 服务问题

### 问题：服务无法启动

**症状**:
```
Error: Service socket not found. Is the service running?
```

**排查步骤**:

1. 检查服务是否运行
```bash
control-tower service status
```

2. 启动服务
```bash
control-tower service start
```

3. 检查日志
```bash
control-tower service logs
```

4. 检查配置文件是否存在
```bash
ls -la ~/.config/control-tower/
cat ~/.config/control-tower/config.yaml
```

**解决方案**:
- 如果配置文件不存在，需要先配置代理
- 如果端口被占用，查找并终止占用进程
- 检查 mihomo 二进制文件是否存在

---

### 问题：服务启动后立即退出

**症状**:
服务看起来启动了但立即退出

**排查步骤**:

1. 查看详细日志
```bash
RUST_LOG=debug control-tower service start
```

2. 手动启动查看错误
```bash
# 找到二进制文件位置
which control-tower-service

# 手动运行查看错误
./target/release/control-tower-service --foreground
```

3. 检查 systemd 日志（如果以服务方式安装）
```bash
sudo journalctl -u control-tower -n 100
```

---

### 问题：IPC 通信失败

**症状**:
```
Error: Failed to communicate with service: Connection refused
```

**排查步骤**:

1. 确认 socket 文件存在
```bash
ls -la ~/.config/control-tower/control-tower-service.sock
```

2. 确认服务进程在运行
```bash
ps aux | grep control-tower
```

3. 检查 socket 权限
```bash
ls -la ~/.config/control-tower/
```

**解决方案**:
- 如果 socket 不存在，重启服务
- 如果权限问题，修改 socket 目录权限

---

## 连接问题

### 问题：connections list 显示 "No active connections"

**排查步骤**:

1. 确认有网络活动
```bash
# 访问一个网站
curl -v https://www.google.com
```

2. 通过浏览器产生一些流量

3. 再次检查连接
```bash
control-tower connections list
```

**注意**: 这可能是正常现象，表示当前没有活动连接

---

### 问题：无法关闭连接 "Connection not found"

**排查步骤**:

1. 列出当前连接
```bash
control-tower connections list
```

2. 使用正确的索引号
```bash
# 注意：索引从 1 开始，不是 0
control-tower connections close 1
control-tower connections close 2
```

---

### 问题：连接详情显示 "Connection has no ID"

**原因**: 连接信息不完整

**排查步骤**:
```bash
# 使用 Web UI 查看更详细的信息
control-tower web
# 然后访问 http://127.0.0.1:8080
```

---

## 代理问题

### 问题：proxy list 为空

**排查步骤**:

1. 确认服务正在运行
```bash
control-tower service status
```

2. 确认配置文件正确
```bash
curl -s http://127.0.0.1:9090/proxies | jq '.proxies | keys'
```

3. 检查订阅是否配置正确
```bash
control-tower profile list
```

---

### 问题：proxy select 失败 "No proxy found matching"

**排查步骤**:

1. 查看所有可用代理
```bash
control-tower proxy list
```

2. 使用确切的名称
```bash
control-tower proxy select "香港 101"
```

3. 使用索引号
```bash
# 查看索引
control-tower proxy list
# 使用索引选择（#号可选）
control-tower proxy select "#7"
```

---

### 问题：proxy test 超时

**排查步骤**:

1. 检查网络连接
```bash
ping -c 3 cp.cloudflare.com
```

2. 确认代理可用
```bash
control-tower proxy list
# 检查节点健康状态
```

3. 增加超时时间（目前是 10 秒）

---

### 问题：proxy stats 显示所有节点 Dead

**排查步骤**:

1. 确认服务正在运行
```bash
control-tower service status
```

2. 测试单个节点
```bash
control-tower proxy test "香港 101"
```

3. 检查防火墙设置
```bash
# 检查 7890 端口
curl -s http://127.0.0.1:9090/proxies | jq '.'
```

---

## 配置问题

### 问题：mode set 失败

**排查步骤**:

1. 检查当前模式
```bash
control-tower mode get
```

2. 使用有效的模式值
```bash
control-tower mode set rule    # 规则模式
control-tower mode set global  # 全局模式
control-tower mode set direct  # 直连模式
```

3. 检查 API 可用性
```bash
curl -s http://127.0.0.1:9090/configs | jq '.mode'
```

---

### 问题：config set 不生效

**原因**: API 配置更改可能不会持久化到文件

**排查步骤**:

1. 直接编辑配置文件
```bash
vim ~/.config/control-tower/config.yaml
```

2. 重启服务使更改生效
```bash
control-tower service restart
```

---

## 网络问题

### 问题：所有流量都不走代理

**排查步骤**:

1. 检查当前模式
```bash
control-tower mode get
```

2. 检查系统代理设置
```bash
echo $http_proxy
echo $https_proxy
```

3. 测试代理是否工作
```bash
curl -s http://127.0.0.1:9090/proxies | jq '.'
```

---

### 问题：特定网站无法访问

**排查步骤**:

1. 检查是否是直连模式
```bash
control-tower mode get
```

2. 查看连接详情
```bash
control-tower connections list
```

3. 检查规则匹配
```bash
# 查看 Web UI 获取详细规则信息
control-tower web
```

---

### 问题：DNS 解析失败

**排查步骤**:

1. 检查是否使用 TUN 模式
```bash
control-tower mode tun status
```

2. 检查 DNS 设置
```bash
cat /etc/resolv.conf
```

3. 尝试切换到规则模式
```bash
control-tower mode set rule
```

---

## TUN 模式问题

### 问题：TUN 模式启用后无法联网

**排查步骤**:

1. 确认 TUN 已启用
```bash
control-tower mode tun status
```

2. 重启服务
```bash
control-tower service restart
```

3. 检查 TUN 设备
```bash
ip addr | grep tun
```

4. 如果仍不工作，禁用 TUN
```bash
control-tower mode tun disable
control-tower service restart
```

---

### 问题：TUN 模式不支持

**原因**: TUN 需要内核支持，非所有系统都支持

**解决方案**:

1. 检查系统支持
```bash
# 检查 tun 模块
lsmod | grep tun

# 检查 /dev/net/tun
ls -la /dev/net/tun
```

2. 加载 tun 模块（如果未加载）
```bash
sudo modprobe tun
```

3. 如果系统不支持，使用规则模式
```bash
control-tower mode set rule
```

---

## 安装问题

### 问题：service install 失败 "Are you running as root?"

**原因**: 写入 systemd 目录需要 root 权限

**解决**:
```bash
sudo control-tower service install
```

---

### 问题：service install 成功但服务不启动

**排查步骤**:

1. 检查服务状态
```bash
sudo systemctl status control-tower
```

2. 查看日志
```bash
sudo journalctl -u control-tower -n 50
```

3. 手动启动测试
```bash
sudo systemctl start control-tower
```

4. 检查二进制文件路径
```bash
cat /etc/systemd/system/control-tower.service
```

---

### 问题：卸载服务失败

**解决**:
```bash
# 手动停止和禁用
sudo systemctl stop control-tower
sudo systemctl disable control-tower

# 手动删除服务文件
sudo rm /etc/systemd/system/control-tower.service

# 重新加载 systemd
sudo systemctl daemon-reload
```

---

## 诊断命令汇总

```bash
# 服务状态
control-tower service status

# 服务日志
control-tower service logs --lines 100

# API 健康检查
curl -s http://127.0.0.1:9090/

# 获取所有代理
curl -s http://127.0.0.1:9090/proxies | jq '.'

# 获取当前连接
curl -s http://127.0.0.1:9090/connections | jq '.'

# 获取配置
curl -s http://127.0.0.1:9090/configs | jq '.'

# 检查端口占用
lsof -i :9090
lsof -i :7890

# 检查进程
ps aux | grep control-tower
ps aux | grep mihomo

# systemd 日志
sudo journalctl -u control-tower -f
```

---

## 获取帮助

如果以上方案都无法解决您的问题：

1. 收集诊断信息
```bash
# 创建诊断报告
cat > ~/control-tower-diagnostic.txt << EOF
Service Status:
$(control-tower service status)

Service Logs:
$(control-tower service logs --lines 50)

API Health:
$(curl -s http://127.0.0.1:9090/ 2>&1)

Connections:
$(curl -s http://127.0.0.1:9090/connections 2>&1)
EOF
```

2. 查看完整日志
```bash
RUST_LOG=debug control-tower service start 2>&1 | tee debug.log
```

3. 提交 Issue 时附上：
   - 诊断报告内容
   - 操作系统版本
   - 复现步骤
   - 预期行为 vs 实际行为
