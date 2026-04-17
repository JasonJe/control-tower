# Control Tower Web UI 全新设计规格说明

## 概述

本文档描述 Control Tower Web 管理界面的全新设计方案，采用极简奢华风格，单文件 HTML 实现。

---

## 1. 视觉风格

### 1.1 设计语言

参考：Linear、Vercel Dashboard、Raycast

核心原则：克制、留白、精致、不张扬。

### 1.2 色彩系统

#### 浅色主题
| Token | 色值 | 用途 |
|-------|------|------|
| `--bg-primary` | `#ffffff` | 主背景 |
| `--bg-secondary` | `#f9fafb` | 次级背景（卡片、侧边栏） |
| `--bg-tertiary` | `#f3f4f6` | 悬停态 |
| `--border` | `#e5e7eb` | 边框 |
| `--text-primary` | `#111827` | 主文字 |
| `--text-secondary` | `#6b7280` | 次级文字 |
| `--text-tertiary` | `#9ca3af` | 占位符文字 |
| `--accent` | `#3b82f6` | 主强调色（蓝） |
| `--accent-hover` | `#2563eb` | 强调色悬停 |
| `--success` | `#22c55e` | 成功/在线 |
| `--warning` | `#f59e0b` | 警告 |
| `--error` | `#ef4444` | 错误/断开 |

#### 深色主题（系统自动适配）
| Token | 色值 | 用途 |
|-------|------|------|
| `--bg-primary` | `#0f0f0f` | 主背景 |
| `--bg-secondary` | `#181818` | 次级背景 |
| `--bg-tertiary` | `#222222` | 悬停态 |
| `--border` | `#2e2e2e` | 边框 |
| `--text-primary` | `#f9fafb` | 主文字 |
| `--text-secondary` | `#9ca3af` | 次级文字 |
| `--text-tertiary` | `#6b7280` | 占位符文字 |
| `--accent` | `#3b82f6` | 主强调色（蓝） |
| `--accent-hover` | `#60a5fa` | 强调色悬停 |
| `--success` | `#22c55e` | 成功/在线 |
| `--warning` | `#f59e0b` | 警告 |
| `--error` | `#ef4444` | 错误/断开 |

### 1.3 字体

- 主字体：`Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`
- 等宽字体（数值/IP）：`"JetBrains Mono", "Fira Code", monospace`

### 1.4 间距系统

基于 4px 网格：
- `xs`: 4px
- `sm`: 8px
- `md`: 16px
- `lg`: 24px
- `xl`: 32px
- `2xl`: 48px

### 1.5 动效

- 所有过渡：`150ms ease`
- 悬停反馈：`opacity 0.8`、`translateY(-1px)`、阴影加深
- 页面切换：淡入淡出 `opacity 200ms ease`
- 按钮点击：`scale(0.98)` 反馈
- 禁止花哨动画，保持克制

### 1.6 圆角

- 小元素（按钮、输入框）：`6px`
- 卡片：`8px`
- 大容器：`12px`

---

## 2. 布局结构

### 2.1 整体布局

```
┌─────────────────────────────────────────────────────┐
│  Logo    "Control Tower"           [主题切换] [状态灯] │  ← Header (56px)
├──────────┬──────────────────────────────────────────┤
│          │                                          │
│  [📊] 首页 │                                          │
│  [🌐] 节点 │           Main Content                  │
│  [📋] 订阅 │                                          │
│  [📏] 规则 │                                          │
│  [🔗] 连接 │                                          │
│  [⚙️] 设置 │                                          │
│          │                                          │
└──────────┴──────────────────────────────────────────┘
   Sidebar        Content Area
   (200px)        (flex: 1)
```

- **Header**：固定高度 56px，左侧 Logo + 名称，右侧主题切换 + 服务状态指示灯
- **Sidebar**：固定宽度 200px，左侧固定，含图标+文字导航项
- **Content**：右侧自适应区域，内边距 32px，最大宽度 1200px 居中

### 2.2 响应式策略

- **桌面 (> 1024px)**：完整侧边栏 + 内容区
- **平板 (768-1024px)**：侧边栏收起为图标模式（64px）
- **手机 (< 768px)**：底部 Tab 导航取代侧边栏

---

## 3. 页面设计

### 3.1 首页仪表盘

**顶部状态卡片横排**（3 列，间距 16px）：

| 卡片 | 内容 |
|------|------|
| 服务状态 | 指示灯 + "运行中" / "已停止" + 运行时间 |
| 当前模式 | 当前模式名称（Rule / Global / Direct） |
| 当前节点 | 当前选中的代理名称 |

**快捷操作区**：
- 启动/停止服务按钮（根据当前状态切换）
- 切换模式按钮组（Rule / Global / Direct，当前模式高亮）

**底部信息**：
- 在线节点数 / 总节点数
- 当前连接数

### 3.2 节点管理页

**搜索栏**：顶部居右，placeholder "搜索节点..."，实时过滤

**节点列表**：
- 默认按延迟从低到高排序
- 每行显示：节点名称 | 类型 | 延迟（颜色指示）| 当前选中标记
- 延迟颜色：`绿 < 100ms`，`黄 < 300ms`，`红 >= 300ms`，`灰 = 超时/不可达`
- 点击行切换节点，显示成功 Toast

**列表为空时**：显示"暂无节点，请检查服务是否运行"

### 3.3 订阅管理页

**添加订阅表单**（顶部卡片）：
- URL 输入框（placeholder: "订阅地址"）
- 名称输入框（placeholder: "名称（可选）"）
- 添加按钮

**订阅列表**：
- 表格布局：名称 | URL（截断显示）| 状态（活跃/非活跃）| 操作
- 活跃订阅行左侧有蓝色竖条指示
- 操作：激活 / 更新 / 删除（删除需二次确认）

### 3.4 规则管理页

**规则列表**：
- 表格布局：序号 | 类型 | 值 | 代理 | 操作
- 操作：删除（按索引）
- 支持分页（每页 50 条）

**添加入口**（顶部）：
- 规则类型下拉 + 值输入 + 代理输入 + 添加按钮

### 3.5 连接监控页

**顶部统计栏**：
- 总连接数 | 上行速率 | 下行速率 | 刷新倒计时（"5s 后刷新"）

**连接列表**：
- 表格布局：序号 | 来源 | 目标 | 代理 | 上行 | 下行 | 时长 | 操作
- 操作：断开（需二次确认）
- 自动刷新间隔 5 秒
- 鼠标悬停在列表上时暂停自动刷新，移出后继续

**列表为空时**：显示"当前无活动连接"

### 3.6 设置页

分区卡片布局：

**服务操作**：
- 重启服务按钮
- 停止服务按钮

**端口信息**（只读展示）：
- HTTP 端口 / SOCKS5 端口 / API 端口

**关于**：
- 版本号

---

## 4. 组件清单

### 4.1 按钮

| 变体 | 样式 |
|------|------|
| Primary | 蓝色填充，白色文字，圆角 6px |
| Secondary | 灰色填充，深色文字 |
| Danger | 红色填充，白色文字 |
| Ghost | 无背景，文字色，悬停时浅灰背景 |

状态：Default / Hover / Active (scale 0.98) / Disabled (opacity 0.5)

### 4.2 输入框

- 高度 36px，圆角 6px，边框 `--border`
- Focus：边框变为 `--accent`，无外阴影
- Placeholder：`--text-tertiary`

### 4.3 卡片

- 背景 `--bg-secondary`，圆角 8px，内边距 20px
- 可选：底部细线分隔（1px `--border`）

### 4.4 表格

- 表头：`--text-secondary`，字号 12px，大写，字重 500
- 行高：48px
- 底部边框分隔（1px `--border`）
- 悬停行：背景 `--bg-tertiary`

### 4.5 Toast 通知

- 固定于页面右下角
- Success：绿色左边框，Error：红色左边框
- 3 秒后自动消失，淡入淡出

### 4.6 导航项

- 高度 36px，圆角 6px
- Default：`--text-secondary` 文字
- Hover：`--bg-tertiary` 背景
- Active：`--bg-secondary` 背景 + `--accent` 文字 + 左侧 2px `--accent` 竖条

---

## 5. 技术实现

### 5.1 文件结构

```
src/
├── web.rs   # Actix Web 服务器 + 路由定义
├── api.rs   # REST API 处理函数（已存在）
└── html.rs  # 单文件 HTML 模板常量
```

### 5.2 HTML 实现

- 单一 `html.rs` 文件，导出 `INDEX_HTML: &str`
- 内联所有 CSS（通过 `<style>` 标签）
- 内联所有 JavaScript（通过 `<script>` 标签）
- 无外部依赖（无 CDN、无框架）
- 使用 CSS 变量实现主题切换
- 使用 `prefers-color-scheme: dark` 媒体查询自动响应系统主题

### 5.3 JavaScript 架构

```
├── State
│   ├── currentView: string
│   ├── theme: 'light' | 'dark' | 'system'
│   ├── serviceStatus: { running, mode, uptime }
│   └── refreshTimer: number
│
├── API Layer
│   ├── api.get(endpoint) → Promise
│   ├── api.post(endpoint, body) → Promise
│   └── api.del(endpoint) → Promise
│
├── Views
│   ├── renderDashboard()
│   ├── renderProxies()
│   ├── renderProfiles()
│   ├── renderRules()
│   ├── renderConnections()
│   └── renderSettings()
│
└── Utils
    ├── formatBytes(n)
    ├── formatUptime(seconds)
    ├── delayColor(ms)
    └── toast(message, type)
```

### 5.4 API 调用

现有 API 端点（来自 `api.rs`）：

| 方法 | 端点 | 用途 |
|------|------|------|
| GET | `/api/service/status` | 服务状态 |
| GET | `/api/proxies` | 代理列表 |
| PUT | `/api/proxies/select` | 选择代理 |
| GET | `/api/profiles` | 订阅列表 |
| POST | `/api/profiles` | 添加订阅 |
| DELETE | `/api/profiles/{id}` | 删除订阅 |
| POST | `/api/profiles/{id}/activate` | 激活订阅 |
| GET | `/api/connections` | 连接列表 |
| DELETE | `/api/connections/{id}` | 关闭连接 |
| GET | `/api/mode` | 获取模式 |
| PUT | `/api/mode` | 设置模式 |
| GET | `/api/config` | 获取配置 |

### 5.5 主题切换实现

```css
:root {
  /* 浅色变量 */
}
@media (prefers-color-scheme: dark) {
  :root {
    /* 深色变量覆盖 */
  }
}
```

JavaScript 控制：`document.documentElement.setAttribute('data-theme', 'dark'|'light')`

---

## 6. 交付物

- `crates/control-tower-cli/src/html.rs`：重写为符合本设计规范的单个 HTML 模板
- `crates/control-tower-cli/src/web.rs`：路由和服务器配置（最小改动）
- `crates/control-tower-cli/src/api.rs`：可能需要新增少量 API 端点（如规则相关）

---

## 7. 范围边界

**包含**：
- 全部 6 个页面的设计和实现（首页、节点、订阅、规则、连接、设置）
- 浅色/深色主题自动切换
- 响应式布局（桌面/平板/手机）
- 所有交互逻辑和数据渲染

**不包含**：
- 后端 API 改造（除必要的新增端点外）
- 认证/权限系统
- 多语言支持
- 离线 PWA 支持
- 暗黑模式手动切换（跟随系统，自动适配）
