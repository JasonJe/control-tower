# Service HTTP API + Web UI 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 将 HTTP API 和 Web UI 从 CLI 下沉到 Service（`ctsvc`），实现 `ctsvc` 默认启动 HTTP API + Web UI，`ctctl` 去除 web 相关功能成为纯 CLI 工具。

**架构：**
- `ctsvc` 进程内置 actix-web HTTP 服务器，默认监听 `settings.yaml` 的 `service_port`（默认 8080）
- HTTP API handlers 直接调用 `ServiceState` 方法（不经过 IPC socket）
- Web UI 为嵌入 `ctsvc` 的单 HTML 文件，路由 `/` 返回 HTML
- `ctctl` 去除 `web` 命令、`api.rs`、`html.rs`、`web.rs` 全部删除

**技术栈：** actix-web 4, actix-rt 2, Rust async/await

---

## 文件结构

```
crates/control-tower-service/
├── src/
│   ├── main.rs              # 修改：添加 HTTP server 初始化和启动
│   ├── api.rs               # 新建：HTTP API handlers（迁移自 CLI）
│   ├── http_server.rs       # 新建：actix-web HTTP server setup
│   └── html.rs              # 新建：Web UI HTML 模板

crates/control-tower-cli/
├── src/
│   ├── main.rs              # 修改：去除 Web 命令
│   ├── web.rs               # 删除
│   ├── api.rs               # 删除（迁移到 service）
│   └── html.rs              # 删除（迁移到 service）
└── Cargo.toml               # 修改：去除 actix-web 依赖
```

---

## 第一阶段：Service 添加 actix-web

### 任务 1：添加 actix-web 依赖到 service

**文件：**
- 修改：`crates/control-tower-service/Cargo.toml`
- 修改：`crates/control-tower-cli/Cargo.toml`

- [ ] **步骤 1：修改 service Cargo.toml，添加 actix-web 依赖**

在 `[dependencies]` 中添加：
```toml
actix-web = { workspace = true }
actix-rt = { workspace = true }
```

从 CLI Cargo.toml 中移除：
```toml
actix-web = { workspace = true }
actix-rt = { workspace = true }
actix-files = { workspace = true }
```

注意：`actix-web = { workspace = true }` 和 `actix-rt = { workspace = true }` 已经在 workspace Cargo.toml 中定义，只需在 service 的 `Cargo.toml` 中添加，CLI 的 `Cargo.toml` 中移除即可。

- [ ] **步骤 2：验证编译**

运行：`cargo build -p control-tower-service`
预期：编译成功，无错误

---

## 第二阶段：Service HTTP API 模块

### 任务 2：创建 `crates/control-tower-service/src/api.rs`

**文件：**
- 创建：`crates/control-tower-service/src/api.rs`

API handlers 直接调用 `Arc<ServiceState>` 方法，使用 `ServiceStatus` 作为返回类型。`Arc<ServiceState>` 通过 actix-web `Data` 传入。

- [ ] **步骤 1：编写 `api.rs` 骨架**

```rust
//! HTTP API handlers for Control Tower Service

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::main::{IpcResponse, ServiceState, ServiceStatus};

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self { code: 0, message: "success".to_string(), data: Some(data) }
    }
    pub fn error(msg: &str) -> ApiResponse<()> {
        ApiResponse { code: -1, message: msg.to_string(), data: None }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetModeRequest { pub mode: String }

#[derive(Debug, Deserialize)]
pub struct SelectProxyRequest { pub name: String }

#[derive(Debug, Deserialize)]
pub struct AddProfileRequest { pub url: String, pub name: Option<String> }
```

- [ ] **步骤 2：实现 `service_status` handler**

```rust
pub async fn service_status(state: web::Data<Arc<ServiceState>>) -> HttpResponse {
    let status = state.status();
    HttpResponse::Ok().json(ApiResponse::success(&status))
}
```

- [ ] **步骤 3：实现 `get_proxies` handler**

```rust
pub async fn get_proxies(state: web::Data<Arc<ServiceState>>) -> HttpResponse {
    match state.get_proxies() {
        Ok(data) => HttpResponse::Ok().json(ApiResponse::success(&data)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}
```

- [ ] **步骤 4：实现 `select_proxy` handler**

```rust
pub async fn select_proxy(
    state: web::Data<Arc<ServiceState>>,
    req: web::Json<SelectProxyRequest>,
) -> HttpResponse {
    match state.select_proxy(&req.name) {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}
```

- [ ] **步骤 5：实现 `get_mode` handler**

ServiceState 不直接存储 mode，但可以通过 Mihomo API 获取。复用 `get_proxies` 的结果解析 mode，或直接调用 Mihomo API：

```rust
pub async fn get_mode(state: web::Data<Arc<ServiceState>>) -> HttpResponse {
    match state.get_proxies() {
        Ok(proxies) => {
            let mode = proxies.get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("rule");
            HttpResponse::Ok().json(ApiResponse::success(mode))
        }
        Err(_) => HttpResponse::Ok().json(ApiResponse::success("rule")),
    }
}
```

- [ ] **步骤 6：实现 `set_mode` handler**

```rust
pub async fn set_mode(
    state: web::Data<Arc<ServiceState>>,
    req: web::Json<SetModeRequest>,
) -> HttpResponse {
    let valid = ["rule", "global", "direct"];
    if !valid.contains(&req.mode.as_str()) {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Invalid mode"));
    }
    match state.set_mode(&req.mode) {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}
```

- [ ] **步骤 7：实现 `get_connections` handler**

```rust
pub async fn get_connections(state: web::Data<Arc<ServiceState>>) -> HttpResponse {
    match state.get_connections() {
        Ok(data) => HttpResponse::Ok().json(ApiResponse::success(&data)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}
```

- [ ] **步骤 8：实现 `close_connection` handler**

```rust
pub async fn close_connection(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let id = path.into_inner();
    match state.close_connection(&id) {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}
```

- [ ] **步骤 9：实现 `get_config` handler（读取 verge.yaml）**

使用 `ControlTowerPaths::from_settings` 解析路径，读取 `verge.yaml`：

```rust
use control_tower_service_core::ControlTowerPaths;

pub async fn get_config() -> HttpResponse {
    // Find settings path and resolve config dir
    let settings_path = find_settings_path().unwrap_or_else(|| PathBuf::from("settings.yaml"));
    let paths = ControlTowerPaths::from_settings(settings_path, None);
    let verge_path = paths.verge_config_path;

    if !verge_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({})));
    }

    match std::fs::read_to_string(&verge_path) {
        Ok(content) => {
            let json: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap_or_default();
            HttpResponse::Ok().json(ApiResponse::success(&json))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
    }
}
```

需要 `find_settings_path()` 函数，逻辑同 CLI `settings.rs` 中的实现（优先检查当前目录、exe 同目录、~/.config/control-tower）。

- [ ] **步骤 10：实现 profile 相关 handlers**

Profiles.yaml 读写需要路径信息。`ServiceState` 已有 `load_cron_jobs()` 使用 `exe_dir` 解析路径。对于 API，需要使用 `ControlTowerPaths`：

**`GET /api/profiles`** — 读取 profiles.yaml：
```rust
pub async fn get_profiles() -> HttpResponse {
    let paths = get_control_tower_paths();
    let profiles_path = paths.profiles_path;

    if !profiles_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(vec::<serde_json::Value>::new()));
    }

    match std::fs::read_to_string(&profiles_path) {
        Ok(content) => {
            // Parse profiles.yaml similar to CLI's load_profiles()
            // Return Vec<Profile> as JSON
            let profiles = parse_profiles_yaml(&content);
            HttpResponse::Ok().json(ApiResponse::success(&profiles))
        }
        Err(e) => HttpResponse::Ok().json(ApiResponse::success(vec::<serde_json::Value>::new())),
    }
}
```

**`POST /api/profiles`** — 下载订阅并保存：
```rust
pub async fn add_profile(req: web::Json<AddProfileRequest>) -> HttpResponse {
    // Download subscription via reqwest, save to profiles dir
    // Add entry to profiles.yaml
    // Return created profile
}
```

**`POST /api/profiles/{id}/activate`** — 激活订阅：
```rust
pub async fn activate_profile(path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    // 1. Update profiles.yaml current marker
    // 2. Use ActiveConfigStore::replace_from_profile(profile_file) to replace config.yaml
    // Return success
}
```

**`DELETE /api/profiles/{id}`** — 删除订阅：
```rust
pub async fn delete_profile(path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    // Remove from profiles.yaml and delete profile file
}
```

- [ ] **步骤 11：实现 rule 相关 handlers（新增）**

Rule handlers 是全新的 API，需要操作 `config.yaml` 中的 rules 数组：

```rust
// GET /api/rules — 读取 config.yaml 中的 rules
pub async fn get_rules() -> HttpResponse {
    let store = ActiveConfigStore::new(get_control_tower_paths());
    // ActiveConfigStore 需要暴露读取 rules 的方法
    // 如果没有，从 config.yaml 直接读取
}

// POST /api/rules — 添加规则
// DELETE /api/rules/{index} — 删除规则（按索引，1-based）
```

注意：`ActiveConfigStore` 当前只有写入方法，需要新增只读的 rules 读取方法或公开 `rules` 字段。

- [ ] **步骤 12：实现 `service_start` 和 `service_stop` handlers**

```rust
pub async fn service_start(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    // Get default config path from ControlTowerPaths
    let paths = get_control_tower_paths();
    let config_path = paths.active_config_path;

    match state.start(&config_path) {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}

pub async fn service_stop(state: web::Data<Arc<ServiceState>>) -> HttpResponse {
    match state.stop() {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}
```

- [ ] **步骤 13：实现 `proxy_delay` handler（测试延迟）**

```rust
// POST /api/proxy/delay
pub async fn proxy_delay(req: web::Json<ProxyDelayRequest>) -> HttpResponse {
    let name = &req.name;
    let url = req.url.as_deref().unwrap_or("http://cp.cloudflare.com/generate_204");
    let timeout = req.timeout.unwrap_or(5000);

    let delay = test_proxy_delay(name, url, timeout).await;
    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": delay })))
}
```

`test_proxy_delay` 使用 `reqwest::Client` 异步调用 Mihomo 的 `/proxies/{name}/delay` 端点。

- [ ] **步骤 14：验证编译**

运行：`cargo build -p control-tower-service`
预期：编译成功

---

## 第三阶段：HTTP Server 模块

### 任务 3：创建 `crates/control-tower-service/src/http_server.rs`

**文件：**
- 创建：`crates/control-tower-service/src/http_server.rs`

- [ ] **步骤 1：编写 HTTP server 启动函数**

```rust
//! HTTP server setup for Control Tower Service

use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use std::sync::Arc;

use crate::api;
use crate::main::ServiceState;

/// Start the HTTP API server on the given port.
/// This runs in the same process as the service.
pub async fn start_http_server(port: u16, state: Arc<ServiceState>) -> std::io::Result<()> {
    tracing::info!("Starting HTTP API server on http://0.0.0.0:{}", port);

    let state_data = web::Data::new(state);

    HttpServer::new(move || {
        App::new()
            .app_data(state_data.clone())
            .wrap(middleware::Logger::default())
            // Web UI
            .route("/", web::get().to(index))
            // API routes
            .service(
                web::scope("/api")
                    .route("/profiles", web::get().to(api::get_profiles))
                    .route("/profiles", web::post().to(api::add_profile))
                    .route("/profiles/{id}", web::delete().to(api::delete_profile))
                    .route("/profiles/{id}/activate", web::post().to(api::activate_profile))
                    .route("/mode", web::get().to(api::get_mode))
                    .route("/mode", web::put().to(api::set_mode))
                    .route("/proxies", web::get().to(api::get_proxies))
                    .route("/proxies/select", web::put().to(api::select_proxy))
                    .route("/connections", web::get().to(api::get_connections))
                    .route("/connections/{id}", web::delete().to(api::close_connection))
                    .route("/service/status", web::get().to(api::service_status))
                    .route("/service/start", web::post().to(api::service_start))
                    .route("/service/stop", web::post().to(api::service_stop))
                    .route("/config", web::get().to(api::get_config))
                    .route("/rules", web::get().to(api::get_rules))
                    .route("/rules", web::post().to(api::add_rule))
                    .route("/rules/{index}", web::delete().to(api::delete_rule))
                    .route("/proxy/delay", web::post().to(api::proxy_delay))
            )
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}

async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(crate::html::INDEX_HTML)
}
```

---

## 第四阶段：Web UI HTML

### 任务 4：创建 `crates/control-tower-service/src/html.rs`

**文件：**
- 创建：`crates/control-tower-service/src/html.rs`

- [ ] **步骤 1：创建 html.rs 模板文件**

```rust
//! Embedded Web UI HTML template

pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
... entire HTML content ...
"#;
```

HTML 内容根据设计规格 `docs/superpowers/specs/2026-04-08-web-ui-redesign-design.md` 实现，包含：
- CSS 变量定义（浅色/深色主题）
- 响应式布局（侧边栏 + 内容区）
- 6 个页面：首页、节点、订阅、规则、连接、设置
- JavaScript API 调用层（调用 `/api/*` 端点）

具体 HTML/CSS/JS 实现根据设计规格编写，确保：
- 所有颜色使用 CSS 变量
- 主题通过 `@media (prefers-color-scheme: dark)` 自动切换
- 无任何外部 CDN 或框架依赖
- API_BASE 指向 `/api`

---

## 第五阶段：Service Main 集成

### 任务 5：修改 `crates/control-tower-service/src/main.rs` 集成 HTTP server

**文件：**
- 修改：`crates/control-tower-service/src/main.rs`

- [ ] **步骤 1：在 main 函数中启动 HTTP server**

在 `main()` 函数中，`ServiceState` 初始化之后、IPC server 启动之前，添加 HTTP server 启动。

查找当前的 main 函数结构，在 `ServiceState` 创建后添加：

```rust
// 在 tokio::spawn 启动 IPC server 之前添加：
let service_port = get_service_port();
let http_state = state.clone();
let http_handle = tokio::spawn(async move {
    if let Err(e) = http_server::start_http_server(service_port, http_state).await {
        tracing::error!("HTTP server error: {}", e);
    }
});
```

- [ ] **步骤 2：添加 `get_service_port()` 函数**

在 `main.rs` 中添加（读取 settings.yaml 中的 `service_port`，默认 8080）：

```rust
fn get_service_port() -> u16 {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    let settings_path = exe_dir
        .map(|p| p.join("settings.yaml"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("settings.yaml"));

    if settings_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.service_port.unwrap_or(8080);
            }
        }
    }
    8080 // default
}

#[derive(serde::Deserialize)]
struct Settings {
    service_port: Option<u16>,
}
```

- [ ] **步骤 3：添加 `mod api; mod http_server; mod html;` 声明**

在 `main.rs` 顶部 `mod scheduler;` 后添加：
```rust
mod api;
mod http_server;
mod html;
```

- [ ] **步骤 4：验证编译**

运行：`cargo build -p control-tower-service`
预期：编译成功，无错误

---

## 第六阶段：CLI 去除 Web 相关功能

### 任务 6：修改 `crates/control-tower-cli/src/main.rs` 去除 Web 命令

**文件：**
- 修改：`crates/control-tower-cli/src/main.rs`

- [ ] **步骤 1：删除 `Web` 枚举变体和相关字段**

在 `Commands` 枚举中删除：
```rust
/// Start Web UI server
Web {
    /// Host to bind (default: 0.0.0.0)
    #[arg(long, default_value = "0.0.0.0")]
    host: String,
    /// Port to bind (default: 8080)
    #[arg(short, long, default_value = "8080")]
    port: u16,
},
```

同时删除 `main()` match 分支中的 `Commands::Web { host, port } => web::start_web_server(&host, port).await?,`

- [ ] **步骤 2：删除 `web` 模块的 import**

在 `main.rs` 顶部删除 `mod web;` 和 `mod html;`（如果存在）。

- [ ] **步骤 3：验证编译**

运行：`cargo build -p control-tower-cli`
预期：编译成功，`ctctl web` 命令不再存在

---

### 任务 7：删除 CLI 的 web、api、html 相关文件

**文件：**
- 删除：`crates/control-tower-cli/src/web.rs`
- 删除：`crates/control-tower-cli/src/api.rs`
- 删除：`crates/control-tower-cli/src/html.rs`

- [ ] **步骤 1：删除文件**

使用 `rm` 或 git rm 删除上述三个文件。

- [ ] **步骤 2：确认 CLI 仍可编译运行**

运行：`cargo build -p control-tower-cli && ./target/debug/ctctl --help`
预期：`--help` 输出中不再包含 `web` 命令

---

## 第七阶段：测试

### 任务 8：测试 HTTP API 功能

- [ ] **步骤 1：启动 ctsvc**

运行：`cargo run -p control-tower-service`
预期：服务启动，HTTP API 监听在 8080 端口

- [ ] **步骤 2：测试 API 端点**

```bash
curl http://127.0.0.1:8080/api/service/status
curl http://127.0.0.1:8080/api/proxies
curl http://127.0.0.1:8080/api/profiles
curl http://127.0.0.1:8080/api/mode
```

预期：返回 JSON 格式的响应

- [ ] **步骤 3：测试 Web UI 访问**

浏览器访问：`http://127.0.0.1:8080/`
预期：显示 Web UI 页面

---

## 第八阶段：收尾

### 任务 9：更新 API 文档

**文件：**
- 修改：`docs/API.md`

更新端点列表，添加新的 API 端点（rules、service/start、service/stop、proxy/delay）。

### 任务 10：Commit

按任务分 3 个 commit：
1. Service HTTP API 基础设施（actix-web + api.rs + http_server.rs）
2. Web UI HTML + Service main 集成
3. CLI 去除 web 功能
