# Web UI 功能增强规格

## 功能 1：Profile 显示最近拉取时间

- **位置**: profiles 表格的 `updated_at` 列
- **数据源**: `profiles.yaml` 的 `updated_at` (Unix timestamp)
- **格式**: `YYYY-MM-DD HH:mm`（如 `2026-04-19 15:30`）
- **实现**: 前端 JS 格式化 Unix 时间戳

## 功能 2：Connections 页面累计流量

- **位置**: Connections stat-grid，新增两个 stat card
- **新增卡片**:
  - `Cumulative Upload` — 累计上传总流量，格式化 `xx.x MB / GB`
  - `Cumulative Download` — 累计下载总流量，格式化 `xx.x MB / GB`
- **数据**: 从 `GET /api/connections` 返回的每条 connection 的 `upload`/`download` 字段求和
- **格式化**: 小于 1MB 显示 `KB`，1MB-1GB 显示 `MB`，大于 1GB 显示 `GB`

## 功能 3：Settings 页面日志展示

- **位置**: Settings 页面底部，可折叠区域，默认展开
- **Tab**: `ctsvc` / `mihomo` 两个切换
- **内容区**: 固定高度 300px，超出滚动，自动滚动到底部
- **刷新**: 手动刷新按钮 + 自动刷新（可配置间隔）
- **间隔选择**: 下拉 5s / 10s / 30s / 60s / Manual（默认 30s）
- **API**: `GET /api/logs?source=ctsvc&lines=200` 和 `GET /api/logs?source=mihomo&lines=200`

## 功能 4：Connections 自定义刷新间隔

- **位置**: Connections header，Pause 按钮左侧
- **控件**: 下拉选择器
- **选项**: `3s` / `5s` / `10s` / `30s` / `Manual`
- **联动**: "Next Refresh" stat card 显示当前选中间隔，倒计时逻辑按间隔调整
- **默认**: `5s`（保持现有行为）

## API 变更

### 新增 `GET /api/logs`
- Query: `source=ctsvc|mihomo`, `lines=N`（默认 200）
- Response: `{ items: [string], source: string, total_lines: number }`
- 实现: 读取 `/opt/ctsvc/logs/ctsvc.log.*` 和 `/opt/ctsvc/logs/mihomo.log`，返回最后 N 行
