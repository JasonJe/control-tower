# 硬编码地址清单

## 已完成修复

### 后端修复（Rust）
- ✅ main.rs: 所有 Mihomo API 调用已改用 `self.get_api_url()` 动态获取
- ✅ api.rs: proxy_delay, proxy_delay_post 已改用 `state.get_api_url()`
- ✅ ServiceState: 新增 `api_host`/`api_port` 字段，从 settings.yaml 读取
- ✅ ServiceState: 新增 `ensure_settings_file()` 启动时自动创建默认 settings.yaml
- ✅ ServiceState: 新增 `update_config_ports()` 更新 config.yaml 端口
- ✅ ServiceState: 新增 `apply_port_settings()` 保存设置+更新config+重启Mihomo

### API 端点
- ✅ `GET /api/settings` - 返回当前配置
- ✅ `PUT /api/settings` - 保存配置到 settings.yaml
- ✅ `POST /api/settings/apply-ports` - 应用端口修改到 config.yaml 并重启 Mihomo

### 前端修复
- ✅ Settings 页面支持编辑 Mihomo 端口
- ✅ loadSettings() 从 API 读取当前配置并显示
- ✅ savePorts() 调用 apply-ports 接口
- ✅ 编辑态使用独立的 port-* CSS 类，不影响其他页面

### CLI 硬编码（暂不处理，影响小）
- ⚠️ cli/src/proxy.rs: CLASH_API_HOST, CLASH_PROXY_PORT 常量
- ⚠️ cli/src/service.rs: println! 打印的 API 地址
- ⚠️ cli/src/mode.rs: CLASH_API_HOST 常量
- ⚠️ cli/src/connections.rs: println! 打印的 web UI 地址

## 端口修改流程

1. 用户在 Settings 页面修改端口 → 点击 Save
2. `savePorts()` 调用 `PUT /api/settings` 保存到 settings.yaml
3. `savePorts()` 调用 `POST /api/settings/apply-ports`
4. 后端执行 `apply_port_settings()`:
   - 更新 `config.yaml` 的 `mixed-port` 和 `socks-port`
   - 重启 Mihomo（如果正在运行）
5. 如果 Mihomo 未运行，只更新 config.yaml，用户需手动启动
