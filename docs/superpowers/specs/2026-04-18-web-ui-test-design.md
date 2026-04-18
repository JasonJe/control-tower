# Control Tower Web UI 自动化测试设计方案

## 背景与目标

Control Tower 的 Web UI 需要一套自动化测试覆盖所有功能点，确保每个页面、每个交互都能正确工作。当前没有任何 UI 自动化测试，每次功能迭代都依赖手动验证。

本方案使用 **Playwright** (Python) 对 Web UI 进行全量浏览器自动化测试，覆盖 6 个页面、全部交互细节。

## 环境与约束

- **Playwright**: 1.57.0 (Python, `/home/jason/myenv`)
- **浏览器**: Chromium Headless Shell (已安装)
- **测试对象**: `http://localhost:8080` (ctsvc HTTP API + Web UI)
- **ctsvc 管理**: `conftest.py` 负责启动/停止 ctsvc 进程
- **Mihomo 生命周期**: 由 ctsvc 的 `service start` API 启动，测试套件统一管理

## 测试架构

### 技术栈
- **Playwright Python** (与 JS 版本 API 一致，Python 环境已就绪)
- **pytest** (测试运行框架)
- **Page Object Pattern** (页面对象，降低 UI 变更脆弱性)

### 目录结构
```
test_web_ui/
├── conftest.py              # pytest session fixture: ctsvc lifecycle, browser setup
├── pages/
│   ├── __init__.py
│   ├── base.py              # BasePage: 导航、toast、截图、滚动等通用能力
│   ├── dashboard.py         # DashboardPage
│   ├── proxies.py           # ProxiesPage
│   ├── profiles.py          # ProfilesPage
│   ├── rules.py             # RulesPage
│   ├── connections.py       # ConnectionsPage
│   └── settings.py          # SettingsPage
├── test_navigation.py        # 全局导航、header 状态、移动端布局
└── test_all_pages.py         # 6 个页面完整功能测试
```

## Session 管理 (conftest.py)

### ctsvc Lifecycle Fixture
- **session scope**: 整个测试 session 只启动一次 ctsvc
- **启动**: 启动 `ctsvc` 进程 → 等待 HTTP 端口 8080 就绪 → 确保 Mihomo 未运行（干净态）
- **teardown**: 测试结束后强制 kill ctsvc 和可能残留的 Mihomo 进程
- **Mihomo 准备**: 每个需要 Mihomo 的测试前调用 `POST /api/service/start`，测试后清理

### Browser Fixture
- `browser`: Chromium 实例（headless，默认）
- `context`: 独立浏览器上下文（隔离 cookie/storage）
- `page`: 默认新标签页

## 各页面测试点

### Dashboard (12 项)
| # | 测试项 | 操作 | 预期结果 |
|---|--------|------|---------|
| 1 | 页面加载 | 打开 Dashboard | Status/Mode/Proxy cards 可见，按钮正常 |
| 2 | Service Start | 点击 Start 按钮 | toast "Service started"，按钮状态变化 |
| 3 | Service Stop | 点击 Stop 按钮 | toast "Service stopped" |
| 4 | Start 按钮 disabled | 服务运行中点击 Start | 按钮 disabled（不再可点）|
| 5 | Rule 模式 | 点击 Rule 按钮 | toast "Mode set to rule"，Mode card 更新 |
| 6 | Global 模式 | 点击 Global 按钮 | toast "Mode set to global" |
| 7 | Direct 模式 | 点击 Direct 按钮 | toast "Mode set to direct" |
| 8 | 刷新数据 | F5 刷新页面 | 数据重新加载，状态保持 |
| 9 | Status dot 红色 | 服务停止时 | header 的 status-dot 为红色 (`.stopped`) |
| 10 | Status dot 绿色 | 服务运行时 | status-dot 为绿色 |
| 11 | Quick Stats | 服务运行后 | Online Nodes / Connections 数量显示 |
| 12 | 导航切换 | 点击 Proxies nav-item | Proxies view 显示，nav-item active |

### Proxies (9 项)
| # | 测试项 | 操作 | 预期结果 |
|---|--------|------|---------|
| 1 | 加载状态 | 打开 Proxies | 显示 spinner + "Loading proxies..." |
| 2 | 代理列表渲染 | 数据加载后 | Name/Type/Latency/Status 四列表格 |
| 3 | 搜索过滤 | 输入代理名关键词 | 列表实时过滤 |
| 4 | 清空搜索 | 清空输入框 | 恢复完整列表 |
| 5 | 选择代理 | 点击某行代理 | toast "Selected: xxx"，Selected badge 出现 |
| 6 | 空搜索结果 | 搜索不存在关键词 | 空状态 "No proxies found" |
| 7 | 延迟着色 | 延迟 <100ms | 绿色文字 (.latency-good) |
| 8 | 延迟着色 | 100ms ≤ 延迟 < 300ms | 黄色文字 (.latency-medium) |
| 9 | 延迟超时 | 代理不可达 | 灰色 "Timeout" |

### Profiles (13 项)
| # | 测试项 | 操作 | 预期结果 |
|---|--------|------|---------|
| 1 | 页面加载 | 打开 Profiles | 显示表格或空状态 |
| 2 | 表单验证 | URL 留空提交 | HTML5 原生验证阻止提交 |
| 3 | 添加 profile | 填写 URL + Name 提交 | toast "Profile added"，列表出现新项 |
| 4 | URL 不可达 | 填写不可达 URL | toast 错误 "Failed to download..." |
| 5 | Activate | 点击 Activate | toast "Profile activated"，Active badge 显示 |
| 6 | Delete modal | 点击 Delete | 弹出确认 modal |
| 7 | Delete 取消 | 点击 Cancel | modal 关闭，profile 保留 |
| 8 | Delete 确认 | 点击 Delete 确认 | modal 关闭，profile 从列表消失 |
| 9 | 重复 Activate | 切换激活对象 | Active badge 正确跟随 |
| 10 | Profile 内容 | 查看列表 | Name / URL / Status / Operations 列显示正确 |
| 11 | 刷新持久 | 刷新页面 | profiles.yaml 数据保留 |
| 12 | Profile 续表 | 多个 profile | 按添加顺序显示 |
| 13 | 激活状态标记 | Active profile | 左侧蓝色边框 (`.profile-active`) |

### Rules (14 项)
| # | 测试项 | 操作 | 预期结果 |
|---|--------|------|---------|
| 1 | 页面加载 | 打开 Rules | 显示规则表格或空状态 |
| 2 | 添加规则 | 填写 TYPE/VALUE/PROXY 提交 | toast "Rule added"，新规则在列表第一行 |
| 3 | 规则格式显示 | 规则行渲染 | TYPE / VALUE / PROXY 分离显示 |
| 4 | Delete 单条 | 点击 X 按钮 | 规则删除，列表更新 |
| 5 | Rule type 下拉 | 点击 type 下拉 | 显示全部 8 种类型选项 |
| 6 | 分页存在 | 添加 51+ 条规则 | 分页控件出现，显示 Page X of Y |
| 7 | 分页翻页 | 点击 Next | 正确翻到下一页 |
| 8 | 清空表单 | 添加成功后 | type/val/proxy 输入框已清空 |
| 9 | 无效删除 | DELETE /api/rules/999 | toast 错误 "Invalid index..." |
| 10 | 空状态 | 删除所有规则 | "No rules found" 空状态 |
| 11 | Toast 自动消失 | 触发任意 toast | 3s 后自动消失 |
| 12 | 跨页面持久 | Rules → Proxies → Rules | 规则列表保留（不重新请求）|
| 13 | 全部删除 | 逐条删除 | 最后一条删除后显示空状态 |
| 14 | 添加后翻页 | 添加后 | 当前页刷新，新规则可见 |

### Connections (7 项)
| # | 测试项 | 操作 | 预期结果 |
|---|--------|------|---------|
| 1 | 页面加载 | 打开 Connections | Stats bar 显示 Total/Upload/Download |
| 2 | 自动刷新 | 等待 5s | 数据自动刷新，countdown 归零后重置 |
| 3 | 刷新暂停 | 鼠标悬停表格 | countdown 暂停 |
| 4 | 刷新恢复 | 鼠标离开表格 | countdown 继续倒计时 |
| 5 | 连接统计 | 有流量时 | Upload/Download 显示实时数值 |
| 6 | 关闭连接 | 点击 X 按钮 | toast "Connection closed" |
| 7 | 空状态 | 无连接时 | "No active connections" 空状态 |

### Settings (5 项)
| # | 测试项 | 操作 | 预期结果 |
|---|--------|------|---------|
| 1 | 页面加载 | 打开 Settings | 显示端口、版本信息 |
| 2 | Restart 按钮 | 点击 Restart | 服务重启 toast |
| 3 | Stop 按钮 | 点击 Stop | 服务停止 toast |
| 4 | 端口信息 | 查看 | HTTP/SOCKS5/API 端口正确显示 |
| 5 | 导航返回 | 点击任意 nav-item | 正确切换 |

### 全局/导航 (6 项)
| # | 测试项 | 操作 | 预期结果 |
|---|--------|------|---------|
| 1 | 6 个 nav-item | 依次点击 | 各自 view 显示，对应 nav-item 高亮 |
| 2 | Header status dot | 服务不同状态 | dot 颜色与实际状态一致 |
| 3 | Toast 位置 | 触发任意 toast | 显示在右下角 |
| 4 | Toast 成功样式 | 成功操作后 | 绿色左边框 (`.toast.success`) |
| 5 | Toast 错误样式 | 失败操作后 | 红色左边框 (`.toast.error`) |
| 6 | 移动端布局 | viewport 设为 375px | sidebar 变为底部 tab bar |

**总计：66 个测试点**

## 执行方式

```bash
# 前置: 启动 ctsvc (测试套件自己管理，这里只是手动参考)
ctsvc &

# 运行全部测试
pytest test_web_ui/ -v

# 只测某个页面
pytest test_web_ui/test_all_pages.py::test_dashboard -v

# 生成 HTML 报告
pytest test_web_ui/ --html=report.html --self-contained-html -v
```

## 错误容忍策略

- **Proxies/Mode/Connections**: Mihomo 不可用时，相关测试标记为 `skip` 或 `xfail`，不导致整体失败（因为测试的是 UI 层，不是 Mihomo 本身）
- **网络超时**: profile add 测试用不可达 URL 时允许 30s timeout
- **进程残留**: teardown 确保所有 ctsvc/mihomo 进程清理干净

## 验收标准

1. 所有 66 个测试点都有对应的 pytest 测试函数
2. `pytest test_web_ui/ -v` 在 ctsvc + Mihomo 正常运行下全部 PASS
3. 页面对象模式，每个页面一个 Python 文件
4. 测试失败时 Playwright 自动截图保存到 `test_web_ui/screenshots/`
5. conftest.py 正确管理 ctsvc 生命周期，无进程残留
