# Control Tower 开发准则

## 环境信息
python环境用 /home/jason/myenv 的虚拟环境

## 修改问题准则

### 定位精准，最小化改动
- 改点要精准，问题在哪里就改哪里，不瞎改无关代码
- 每次只改一个明确的点，改完立即验证，不批量修改多个不相关的内容
- 用 `cargo build` 快速验证编译通过，不确定时就 build
- **定位到问题后，调试为主，禁止深挖！** 多加日志直接验证，不要在代码里反复追踪推理

问题修改后，调用 /control-tower-update skill 更新本地部署

### 大改必须备份
- "大改"定义：重写整个文件、批量替换、多文件同时修改、用 `git checkout` reset 文件
- 大改前必须 `cp <file> /tmp/backup_<date>_<name>.rs` 备份，验证失败能立刻恢复
- 禁止在未备份的情况下对正在开发中的文件做 `git checkout HEAD --` 或 `git reset --hard`

### 消除所有编译 Warning
- `cargo build` 编译时不允许产生任何 warning
- warning 意味着代码死代码或潜在逻辑错误，必须修复
- 在 commit 之前确保 `cargo build 2>&1 | grep warning` 无输出

### 引号问题处理（Rust html.rs）
- `pub const INDEX_HTML: &str = r#"..."#;` 里的 HTML/JS/CSS 混在一起，引号容易产生 Rust 解析歧义
- JS 语句里用 `headers: {'Content-Type': 'application/json'}` 会触发误解析
- CSS 里 `font-family:'JetBrains Mono'` 在 HTML style 属性内也会触发
- 遇到编译错误 "unterminated character literal" 或 "prefix xxx is unknown" 且指向 HTML/JS 行，先检查引号问题
- 修复引号问题后立即 build 验证，不要同时做其他修改

## 日常操作

- `sh build.sh dist` 构建发布包
- 服务进程：先 `ps -ef | grep ctsvc` 查 PID，`kill <PID>` 停止，再用 `./ctsvc &` 启动
- 页面 HTML 调试：服务运行时 `curl -s http://localhost:8080/` 可直接拿到渲染后的页面

## 技术背景

- Mihomo API 字段：`sourceIP`, `host`, `destinationIP`, `destinationPort`, `chains`, `start`（不是 `source`/`target`/`proxy`/`duration`）
- Profile UID 格式：`profile-xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`（多段 dash，inline onclick 拼接时需转义单引号）
- 订阅预验证：HTTP GET → 拒绝 HTML 响应 → YAML 解析 → 检查 `proxies`/`proxy-providers`/`mixed-port` 字段
