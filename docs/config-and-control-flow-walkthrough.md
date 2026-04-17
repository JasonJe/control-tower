# 配置真源与控制链路走读（第二轮）

## 文档目的

本文是对 `docs/crates-backend-walkthrough.md` 的继续补充，重点不再是 crate 级职责概览，而是进一步回答两个直接影响后续修复的问题：

1. **配置真源（Source of Truth）到底是什么。**
2. **CLI / Web / service / Mihomo 之间的控制链路是否统一。**

如果说第一轮走读是在回答“系统大体是怎么组织的”，那么本文要回答的是：

- 哪些地方在绕过既定边界。
- 哪些状态是持久化真相，哪些只是运行态缓存。
- 哪些代码路径会导致行为不一致。
- 后续修复时，应该优先收口哪一层。

---

## 一、先给结论

当前系统最大的问题，不是某个函数实现有 Bug，而是：

### 1. 配置根目录没有统一解析入口

不同入口分别使用了不同的路径解析策略：

- CLI：优先 `settings.working_dir`，否则退回可执行文件目录。
- Web API：直接使用 `~/.config/control-tower`。
- service：大量逻辑直接使用可执行文件目录。
- core：默认也是可执行文件目录，但测试期望又是另一套路径。

结果就是：**同一个系统的不同入口可能操作的不是同一套文件。**

### 2. 控制链路没有统一收口到 service

当前有 3 种控制路径并存：

1. CLI → service IPC
2. CLI → 直接调用 Mihomo HTTP API
3. Web API → 直接调用 Mihomo HTTP API

这说明 service 并没有真正成为“唯一应用服务层”，而只是“部分场景下的一个中间层”。

### 3. 运行态与持久化状态之间没有统一同步模型

当前至少存在以下 4 类状态载体：

- `settings.yaml`
- `profiles.yaml`
- `config.yaml`
- Mihomo HTTP API 当前返回值

它们之间的同步规则不是单向、统一、可推理的，而是分散在多个模块中以“各自补偿”的方式存在。

---

## 二、配置根目录：当前到底是谁说了算

## 2.1 CLI 的配置目录解析

CLI 侧配置目录入口在：

- `crates/control-tower-cli/src/config.rs:10-23`

逻辑是：

1. 如果 `settings.yaml` 中设置了 `working_dir`，就用它。
2. 否则回退到当前可执行文件所在目录。

而 `working_dir` 的读取来自：

- `crates/control-tower-cli/src/settings.rs:234-243`

也就是说，对 CLI 来说：

- **`settings.yaml` 是路径配置入口。**
- **可执行文件目录是默认值。**

### 判断

这套逻辑本身是自洽的，问题不在 CLI 自己，而在于其他入口没有复用它。

---

## 2.2 Web API 的配置目录解析

Web API 的本地配置目录在：

- `crates/control-tower-cli/src/api.rs:44-48`

这里完全绕过了 CLI 的配置解析逻辑，直接写死为：

- `~/.config/control-tower`

### 影响

这意味着只要用户设置了 `settings.working_dir`，CLI 和 Web 就会读写不同目录。

一个最典型的场景是：

- CLI `profile add` 把 profile 写进了“可执行文件目录”下。
- Web `get_profiles` 却从 `~/.config/control-tower/profiles.yaml` 读取。
- 用户看到的就是“CLI 加了 profile，但 Web 页面没有”。

### 结论

Web API 当前不是 CLI 逻辑的 HTTP 封装，而是**另一套独立的数据访问实现**。

这是架构上很危险的信号。

---

## 2.3 service 的配置目录解析

service 内多处逻辑直接依赖可执行文件目录：

- 加载 cron：`crates/control-tower-service/src/main.rs:212-219`
- 更新 profile 后写回 `profiles.yaml`：`crates/control-tower-service/src/main.rs:1775-1781`
- 恢复 `settings.yaml` 中的 `selected_proxy` / `mode`：`crates/control-tower-service/src/main.rs:428-469`
- `Restart` 时默认取 `exe_dir/config.yaml`：`crates/control-tower-service/src/main.rs:591-597`

### 影响

这说明 service 的默认假设是：

- 所有运行相关文件都在 `current_exe().parent()` 下。

这与 CLI 的 `working_dir` 逻辑不兼容。

也就是说，只要用户启用了自定义工作目录，service 就可能：

- 启动时读错 `settings.yaml`
- cron 更新时改错 `profiles.yaml`
- 重启时加载错 `config.yaml`

### 结论

当前 service 并不是“基于统一配置对象启动”的，而是“运行时自己猜路径”。

---

## 2.4 core 的配置目录解析

core 侧 `ServiceConfig` 默认配置在：

- `crates/control-tower-service-core/src/config.rs:21-35`

默认值也是：

- `config_dir = exe_dir`
- `socket_path = /tmp/ctsvc.sock`
- `pid_file = /tmp/ctsvc.pid`

但测试断言却是另一套路径：

- `crates/control-tower-service-core/src/config.rs:78-83`

### 判断

这进一步说明，项目中至少存在过两套路径设计：

1. “可执行文件目录 + `/tmp/ctsvc.sock`”
2. “`clash-verge` 风格目录 + `/tmp/verge/...` socket”

当前实现没有收口到统一方案。

---

## 三、配置文件角色：哪些是业务真源，哪些只是缓存

从代码看，当前系统涉及这些配置 / 状态文件：

- `settings.yaml`
- `profiles.yaml`
- `config.yaml`
- `verge.yaml`

## 3.1 `settings.yaml`

定义见：

- `crates/control-tower-cli/src/settings.rs:15-56`

字段包括：

- `working_dir`
- `mihomo_path`
- `api_port`
- `http_port`
- `socks_port`
- `service_port`
- `tun_enabled`
- `log_level`
- `selected_proxy`
- `mode`

### 当前角色

它混合承担了两类职责：

1. **运行参数与环境配置**
   - 例如端口、日志级别、工作目录。
2. **运行态偏好持久化**
   - 例如 `selected_proxy`、`mode`。

### 问题

这会导致 `settings.yaml` 既像配置文件，又像运行态快照。

例如：

- `selected_proxy` 会在 CLI 选代理后写入：`crates/control-tower-cli/src/proxy.rs:166-168`
- service 启动后又从 `settings.yaml` 恢复代理：`crates/control-tower-service/src/main.rs:406-414`
- mode 也采用同样模式：
  - 写入：`crates/control-tower-cli/src/settings.rs:221-231`
  - 恢复：`crates/control-tower-service/src/main.rs:416-423`

### 判断

`settings.yaml` 当前更像“本地偏好 + 启动参数”的混合物，不适合继续承载更多业务状态。

---

## 3.2 `profiles.yaml`

CLI 侧读写在：

- `crates/control-tower-cli/src/config.rs:25-113`

service 侧自动更新也会直接改它：

- `crates/control-tower-service/src/main.rs:1775-1811`

Web API 也会直接读写它：

- `crates/control-tower-cli/src/api.rs:79-123`
- `crates/control-tower-cli/src/api.rs:162-195`
- `crates/control-tower-cli/src/api.rs:252-295`

### 当前角色

`profiles.yaml` 是订阅元数据真源，保存：

- 当前激活项 `current`
- 各 profile 的 `uid`
- `name`
- `file`
- `url`
- `cron`
- `updated_at`（CLI 路径下有，Web 部分结构未完全保留）

### 问题

不同入口对 `profiles.yaml` 的结构理解不完全一致：

- CLI 的 `Profile` 结构有 `updated_at` 和 `cron`
- Web 里某些局部结构体只保留 `uid/name/file/url`
- `activate_profile` 的 Web 版本只改 `current`，不做 CLI 那套复制 `config.yaml` 和重启动作

### 结论

`profiles.yaml` 是系统最像“业务真源”的文件，但目前被多个入口以不同 schema 和不同行为修改，已经出现分叉风险。

---

## 3.3 `config.yaml`

`config.yaml` 在当前系统里的角色非常关键。

它被用作：

- 当前激活的 Mihomo 配置快照
- 规则编辑的落点文件
- 部分 mode 修改的回写目标

相关位置：

- profile 激活复制目标：`crates/control-tower-cli/src/profile.rs:208-218`
- mode 回写：`crates/control-tower-cli/src/service.rs:217-240`
- rule 直接编辑：`crates/control-tower-cli/src/rule.rs:163-250`
- service 启动配置来源：`crates/control-tower-cli/src/service.rs:292-300`
- service restart 默认路径：`crates/control-tower-service/src/main.rs:591-599`

### 当前角色判断

`config.yaml` 实际上是“当前运行配置”的持久化落点。

但它的来源并不统一：

- 有时由 profile 激活产生。
- 有时由 mode 设置直接修改。
- 有时由 rule 管理直接修改。

### 问题

1. 没有统一写入入口。
2. 不同模块直接原地编辑。
3. 无版本、无原子替换、无集中校验。

这会导致：

- 谁都能改 `config.yaml`
- 但没有任何一层对其完整性负责

---

## 3.4 `verge.yaml`

从目前代码看，`verge.yaml` 主要出现在：

- `crates/control-tower-service-core/src/config.rs:55-58`
- `crates/control-tower-cli/src/api.rs:470-485`

当前 Web API 允许读取它，但在本轮走读涉及的主链路中，并没有看到它承担关键业务真源角色。

### 判断

目前它更像是外围配置文件，而不是本次架构问题的核心。

后续走读可以暂时降低优先级。

---

## 四、控制链路：到底谁在控制 Mihomo

当前项目存在 3 套控制路径。

## 4.1 路径 A：CLI → service IPC

这一条路径主要负责：

- `start_service`
- `stop_service`
- `restart_service`
- `status`

实现位置：

- `crates/control-tower-cli/src/service.rs:285-474`

这是目前最像“标准后端分层”的路径：

- 入口层只发命令。
- service 负责执行。

### 但这里也有问题

#### 问题 1：spawn service daemon 时用了 `--foreground`

- `crates/control-tower-cli/src/service.rs:395-406`

CLI 在后台拉起 service 时，传的是 `--foreground`，只是把 stdout / stderr 丢到 `/dev/null`。

这说明所谓“后台 daemon”其实不是一个严格意义上的 daemon 化过程，而是“前台模式 + 静默输出”。

#### 问题 2：service 不存在时，CLI 会直接退回 Mihomo API 判断

例如：

- `status()` 在没有 socket 时，如果 Mihomo API 可达，会显示 “Running (direct mode, no service daemon)”
- 位置：`crates/control-tower-cli/src/service.rs:475-489`

这说明系统接受一种“绕过 service 直接跑 Mihomo”的状态。

这对工具可用性是友好的，但对架构收口是不利的。

---

## 4.2 路径 B：CLI → 直接调用 Mihomo API

CLI 直接调用 Mihomo API 的位置很多：

- mode：`crates/control-tower-cli/src/mode.rs:46-155`
- service helper：`crates/control-tower-cli/src/service.rs:101-283`
- proxy：`crates/control-tower-cli/src/proxy.rs:49-260`
- connections：`crates/control-tower-cli/src/connections.rs:21-236`

### 影响

这意味着 CLI 并不把 service 当成唯一应用层，而是把 service 当作：

- 一部分进程控制的转发器
- 另一部分运行态控制则直接自己处理

### 典型例子

#### 例 1：mode

- `ctctl mode set` 最终走的是 `service::set_mode()`
- `service::set_mode()` 内部是直接 PATCH Mihomo `/configs`
- 位置：`crates/control-tower-cli/src/service.rs:188-215`

#### 例 2：proxy

- `ctctl proxy list` / `select` / `test` 都直接依赖 Mihomo `/proxies`
- 位置：`crates/control-tower-cli/src/proxy.rs:49-260`

#### 例 3：connections

- `ctctl connections list/detail/close/top` 直接用 Mihomo `/connections`
- 位置：`crates/control-tower-cli/src/connections.rs:21-236`

### 判断

这使得 CLI 同时承担了：

- transport
- 业务编排
- 运行态适配

这不利于：

- 后续让 TUI / Web 共享一套应用逻辑
- 把错误码和行为语义统一

---

## 4.3 路径 C：Web API → 直接调用 Mihomo API

Web API 入口在：

- `crates/control-tower-cli/src/web.rs:12-35`

其 handler 基本都在：

- `crates/control-tower-cli/src/api.rs`

从实现看，Web API 的多数控制动作也是直接调用 Mihomo HTTP API：

- `set_mode`：`crates/control-tower-cli/src/api.rs:317-347`
- `get_proxies`：`crates/control-tower-cli/src/api.rs:350-370`
- `select_proxy`：`crates/control-tower-cli/src/api.rs:372-396`
- `get_connections`：`crates/control-tower-cli/src/api.rs:398-418`
- `close_connection`：`crates/control-tower-cli/src/api.rs:420-439`
- `service_status`：`crates/control-tower-cli/src/api.rs:447-468`

而且 Web API 中 `start_service` / `stop_service` 甚至还不是走 IPC，而是临时占位式实现：

- `start_service`：`crates/control-tower-cli/src/api.rs:531-538`
- `stop_service`：`crates/control-tower-cli/src/api.rs:540-553`

### 结论

Web API 当前并不是 service 的 HTTP 包装层，而是：

- 部分功能直接打 Mihomo API
- 部分功能直接改本地文件
- 部分功能只是占位实现

因此它不是一层稳定的应用服务封装，而更像一组“直接操作底层的 handler 集合”。

---

## 五、最关键的不一致：同名能力，不同入口，行为不同

这是当前最值得后续修复时优先对齐的点。

## 5.1 激活 profile

### CLI 行为

- 更新 `profiles.yaml`
- 复制 profile 文件为 `config.yaml`
- 启动或重启 service

位置：

- `crates/control-tower-cli/src/profile.rs:198-246`

### Web 行为

- 仅修改 `profiles.yaml.current`
- 不复制 `config.yaml`
- 不启动 / 重启 service

位置：

- `crates/control-tower-cli/src/api.rs:252-295`

### 影响

同样叫“activate profile”，CLI 和 Web 实际语义完全不同。

这会让后续任何上层调用者都难以建立稳定预期。

---

## 5.2 mode 切换

### CLI 行为

- 调用 Mihomo `/configs`
- 写 `settings.yaml`
- 再尝试写 `config.yaml`

位置：

- `crates/control-tower-cli/src/service.rs:188-240`

### Web 行为

- 只调用 Mihomo `/configs`
- 不写 `settings.yaml`
- 不写 `config.yaml`

位置：

- `crates/control-tower-cli/src/api.rs:317-347`

### 影响

CLI 设置的 mode 具备部分持久化能力，Web 设置的 mode 则更像运行时临时变更。

这会直接导致：

- Web 改过 mode，service 重启后可能丢失
- CLI 改过 mode，service 启动后又会从 `settings.yaml` 恢复

---

## 5.3 service 状态查询

### CLI 行为

优先通过 IPC 查询 service；如果 service socket 不存在，再退回直接探测 Mihomo API：

- `crates/control-tower-cli/src/service.rs:475-499`

### Web 行为

直接探测 Mihomo API，不依赖 service IPC：

- `crates/control-tower-cli/src/api.rs:447-468`

### 影响

CLI 的 “service status” 和 Web 的 “service status” 实际不是在看同一对象：

- CLI 看的是“控制面 + 运行态”的混合状态
- Web 看的是“运行态可达性”

---

## 六、service 本身是否承担了统一控制职责

从 `control-tower-service` 代码看，service 实际已经具备承接统一控制入口的能力。

它有：

- IPC 协议：`crates/control-tower-service/src/main.rs:18-49`
- ServiceState：`crates/control-tower-service/src/main.rs:167-539`
- 代理查询：`crates/control-tower-service/src/main.rs:313-335`
- 连接查询：`crates/control-tower-service/src/main.rs:338-359`
- 连接关闭：`crates/control-tower-service/src/main.rs:362-381`
- mode 恢复：`crates/control-tower-service/src/main.rs:472-489`
- proxy 恢复：`crates/control-tower-service/src/main.rs:491-508`

### 关键观察

service 已经不是“只负责启停 Mihomo”的薄层。

它其实已经具备：

- 运行态控制
- 运行态读取
- 启动后状态恢复
- cron 调度

只是这些能力没有被 CLI / Web 统一复用，而是和入口层重复实现了。

### 判断

这意味着下一步架构治理的正确方向不是“再造一层”，而是：

- **把已有 service 真正收口成唯一应用服务层。**

---

## 七、后续修复建议（比第一轮更具体）

## 7.1 第一优先级：统一配置目录解析入口

### 目标

让 CLI / Web / service / core 使用同一套路径解析逻辑。

### 建议做法

新增一个统一的路径对象，例如：

- `ControlTowerPaths`

至少包含：

- `config_dir`
- `settings_path`
- `profiles_path`
- `active_config_path`
- `verge_config_path`
- `socket_path`
- `log_dir`
- `pid_file`

然后由所有入口统一使用它。

### 直接替换点

- `crates/control-tower-cli/src/config.rs`
- `crates/control-tower-cli/src/api.rs`
- `crates/control-tower-service/src/main.rs`
- `crates/control-tower-service-core/src/config.rs`

---

## 7.2 第二优先级：把 Web API 改成 service 的 HTTP 封装，而不是底层直连

### 目标

Web 不再直接：

- 读写 `profiles.yaml`
- 调 Mihomo `/configs`、`/proxies`、`/connections`

而是通过统一应用层执行。

### 建议做法

短期最现实的方案：

1. 先让 Web 调用与 CLI 相同的 service helper。
2. 再把 helper 中直接操作 Mihomo API 的逻辑逐步迁回 IPC。

更理想的长期方案：

- Web → application service（本地模块）→ IPC / config store / Mihomo adapter

但以当前代码基础，建议先做第一步，不要一步到位大改。

---

## 7.3 第三优先级：为同名能力建立统一语义

优先对齐以下几个能力：

1. `activate profile`
2. `set mode`
3. `service status`
4. `start / stop / restart service`

### 推荐规则

- 同名动作必须走同一条应用逻辑。
- CLI 和 Web 只能在展示层不同，不能在业务语义上不同。
- 是否持久化、是否 reload、是否重启，都应在 service 层统一决定。

---

## 7.4 第四优先级：把 `config.yaml` 的写入收口

### 当前问题

`config.yaml` 被多个模块直接编辑：

- profile
- rule
- mode
- service restart path

### 建议做法

至少做一个集中入口，例如：

- `ConfigRepository` 或 `ActiveConfigStore`

统一负责：

- 读取当前配置
- 校验结构
- 原子写回
- 必要时做备份

这样后续无论是 rule 变更、mode 变更、profile 激活，都会经过同一写入通道。

---

## 八、建议的下一轮走读主题

在本文基础上，最适合继续深入的方向有两个。

## 方向 A：专门走读 `config.yaml` 变更链路

目标：回答以下问题：

- profile / mode / rule 分别如何修改 `config.yaml`
- 是否有字段覆盖风险
- 是否存在 YAML 结构破坏风险
- 哪些修改是原子性的，哪些不是

优先文件：

- `crates/control-tower-cli/src/profile.rs`
- `crates/control-tower-cli/src/service.rs`
- `crates/control-tower-cli/src/rule.rs`
- `crates/control-tower-service/src/main.rs`

## 方向 B：专门走读 service 协议与状态机

目标：回答以下问题：

- IPC 协议是否足够承载未来统一应用层
- 状态机是否存在非法迁移
- 是否需要新增命令来收口 Web / CLI 入口

优先文件：

- `crates/control-tower-service/src/main.rs`
- `crates/control-tower-service-core/src/state.rs`

---

## 九、结论

第二轮走读后，可以更明确地下结论：

### 当前最核心的问题不是“代码分层不明显”，而是“分层存在，但没有真正生效”。

具体表现为：

1. **配置目录解析分裂**
   - 不同入口操作不同目录。

2. **控制链路分裂**
   - CLI / Web / service / Mihomo 之间存在多条平行控制路径。

3. **同名能力语义分裂**
   - CLI 和 Web 对同一个动作的业务含义不同。

4. **持久化与运行态同步分裂**
   - 有的动作先改文件，有的动作先改运行态，有的动作只改其中一侧。

因此，后续修复不应该从“局部小 Bug”切入，而应该优先完成以下 3 件事：

- 统一路径解析
- 统一控制入口
- 统一状态变更语义

这 3 件事做完之后，再去修 profile、mode、rule、connections 这些模块，会轻松很多。

---

## 附：本轮直接引用的关键代码位置

- CLI 配置目录：`crates/control-tower-cli/src/config.rs:10-23`
- settings 工作目录：`crates/control-tower-cli/src/settings.rs:234-243`
- Web API 配置目录：`crates/control-tower-cli/src/api.rs:44-48`
- Web API 路由：`crates/control-tower-cli/src/web.rs:12-35`
- CLI mode：`crates/control-tower-cli/src/mode.rs:11-155`
- CLI proxy：`crates/control-tower-cli/src/proxy.rs:39-260`
- CLI connections：`crates/control-tower-cli/src/connections.rs:21-236`
- CLI rule：`crates/control-tower-cli/src/rule.rs:69-250`
- CLI service 控制：`crates/control-tower-cli/src/service.rs:243-499`
- Web activate profile：`crates/control-tower-cli/src/api.rs:252-295`
- Web mode：`crates/control-tower-cli/src/api.rs:317-347`
- Web service status：`crates/control-tower-cli/src/api.rs:447-468`
- Web service start/stop：`crates/control-tower-cli/src/api.rs:531-553`
- service 状态对象：`crates/control-tower-service/src/main.rs:167-539`
- service 加载 cron：`crates/control-tower-service/src/main.rs:208-270`
- service 执行 cron：`crates/control-tower-service/src/main.rs:272-311`
- service 获取代理 / 连接 / 关闭连接：`crates/control-tower-service/src/main.rs:313-381`
- service 启动后恢复 proxy / mode：`crates/control-tower-service/src/main.rs:406-508`
- service 重启默认配置路径：`crates/control-tower-service/src/main.rs:591-599`
- service 自动更新 profile 后写回 profiles：`crates/control-tower-service/src/main.rs:1775-1811`
- core 配置：`crates/control-tower-service-core/src/config.rs:21-83`
