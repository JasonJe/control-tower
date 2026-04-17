# `crates/` 目录后端视角走读与修复建议

## 文档目的

本文从资深后端工程师视角走读 `crates/` 目录下的 3 个 Rust crate，目标不是简单描述代码结构，而是回答以下问题：

1. 当前系统的职责边界是否清晰。
2. 配置、运行态、持久化之间的真实数据流是什么。
3. 哪些问题是表层实现问题，哪些问题是架构性问题。
4. 后续继续走读时，应该优先看哪里。
5. 如果准备着手修复，第一批改动应如何分层推进。

本文覆盖的代码范围：

- `crates/control-tower-cli`
- `crates/control-tower-service`
- `crates/control-tower-service-core`

---

## 一、总体架构判断

从职责划分看，当前项目大体可以拆成 3 层：

- **入口层：** `control-tower-cli`
- **应用服务层：** `control-tower-service`
- **核心能力层：** `control-tower-service-core`

对应关系如下：

| 层级 | crate | 主要职责 | 当前状态 |
|---|---|---|---|
| 入口层 | `control-tower-cli` | 解析 CLI / Web 请求，读写本地配置，向 service 或 Mihomo 发控制命令 | 边界偏宽，知道太多运行细节 |
| 应用服务层 | `control-tower-service` | 承接 IPC 命令，编排进程生命周期、日志、定时任务、状态查询 | 可运行，但同步阻塞味道较重 |
| 核心能力层 | `control-tower-service-core` | 封装 Mihomo 进程、状态、安装与配置路径 | 仍是“领域逻辑 + 基础设施副作用”混合体 |

### 总体评价

优点：

- crate 切分直观，第一次接手时容易建立心智模型。
- 关键链路较短，排障时容易落到具体文件。
- 本地文件落盘较多，便于手工检查运行状态。
- 已经有一定的稳定性意识，例如 `CircuitBreaker`。

主要问题：

- **配置真源不唯一。**
- **CLI 层承担了部分应用层职责。**
- **core 层不够纯，职责偏胖。**
- **service 使用阻塞式循环，扩展空间有限。**
- **部分测试与实现已出现漂移。**

---

## 二、各 crate 走读结论

## 2.1 `control-tower-cli`

### 2.1.1 入口与模块划分

CLI 入口在 `crates/control-tower-cli/src/main.rs:22-79`，通过 `clap` 定义子命令并分发到各模块：

- `profile`
- `proxy`
- `mode`
- `service`
- `rule`
- `connections`
- `update`
- `web`

这部分结构是清楚的，入口层没有明显的“巨型 main”问题。

### 2.1.2 配置体系

配置读取集中在：

- `crates/control-tower-cli/src/settings.rs:15-56`
- `crates/control-tower-cli/src/settings.rs:108-151`
- `crates/control-tower-cli/src/config.rs:10-23`
- `crates/control-tower-cli/src/config.rs:25-113`

当前的配置搜索优先级是：

1. `--config`
2. 可执行文件同目录 `settings.yaml`
3. `~/.config/control-tower/settings.yaml`

`settings.rs` 使用全局单例保存配置：

- `crates/control-tower-cli/src/settings.rs:81-87`

这意味着：

- CLI 模式下问题不大。
- 一旦复用到 Web 模式，配置会变成进程级共享状态。
- 文件内容与内存缓存可能出现漂移。

### 2.1.3 关键业务链路

#### Profile 管理

`profile` 模块是当前 CLI 中最核心的业务模块，主要逻辑位于：

- `crates/control-tower-cli/src/profile.rs:40-77`：新增 profile
- `crates/control-tower-cli/src/profile.rs:159-195`：更新 profile
- `crates/control-tower-cli/src/profile.rs:198-246`：激活 profile
- `crates/control-tower-cli/src/profile.rs:115-156`：删除 profile

核心流程为：

1. 下载订阅内容。
2. 写入 `profiles/<id>.yaml`。
3. 更新 `profiles.yaml`。
4. 激活时复制为 `config.yaml`。
5. 通过 service 启动或重启 Mihomo。

这是一个“文件系统驱动的配置发布”模型，简单直接，但存在明显问题：

- `activate_profile` 里是直接复制 profile 文件到 `config.yaml`，没有版本控制和原子替换。
- `remove_profile` 在删除 profile 后会继续删除 `config.yaml` 并停止 service，位置在 `crates/control-tower-cli/src/profile.rs:143-155`。
- 删除 profile、删除运行配置、停止 service 被硬绑在一起，属于高耦合设计。

#### Service 交互

本地 service 的 IPC 调用集中在：

- `crates/control-tower-cli/src/service.rs:27-45`
- `crates/control-tower-cli/src/service.rs:53-97`

这里定义了一套本地 `IpcCommand`，通过 Unix socket `/tmp/ctsvc.sock` 与 `control-tower-service` 通信。

这部分边界是合理的：**CLI 不直接管理 Mihomo 子进程，而是通过 service 间接管理。**

#### 直接调用 Mihomo API

但在同一模块中，CLI 也直接调用 Mihomo 的 HTTP API：

- `crates/control-tower-cli/src/service.rs:101-122`：获取代理
- `crates/control-tower-cli/src/service.rs:148-215`：读取 / 设置 mode
- `crates/control-tower-cli/src/proxy.rs`：代理选择与查询
- `crates/control-tower-cli/src/mode.rs`：模式切换
- `crates/control-tower-cli/src/connections.rs`：连接管理

这意味着 CLI 层同时承担两类职责：

1. 调本地 service
2. 直连 Mihomo 运行态 API

这会带来架构上的边界泄漏：

- CLI 知道太多运行态细节。
- Web 端很可能复制一套类似逻辑。
- 业务编排无法统一收口在 service。

### 2.1.4 Web API 与 CLI 配置逻辑不一致

`crates/control-tower-cli/src/api.rs:44-48` 中 `get_config_dir()` 直接使用：

- `~/.config/control-tower`

而 CLI 侧 `crates/control-tower-cli/src/config.rs:10-23` 优先使用 `settings.working_dir`，否则退回可执行文件目录。

这说明：

- **CLI 与 Web 对“配置目录”的理解不一致。**
- 同一个程序的两种入口可能读写不同目录。
- 最终会导致 profile、settings、config.yaml、运行态之间出现难以复现的状态漂移。

这是当前最值得优先处理的问题之一。

---

## 2.2 `control-tower-service`

### 2.2.1 服务定位

`control-tower-service` 是本地常驻控制服务，本质是一个 Unix socket IPC server。

协议定义在：

- `crates/control-tower-service/src/main.rs:18-49`

支持的命令包括：

- `Start`
- `Stop`
- `Restart`
- `Status`
- `Logs`
- `GetProxies`
- `GetConnections`
- `CloseConnection`
- `ReloadCron`

从职责上看，它是典型的“本地控制面应用服务”。

### 2.2.2 命令处理与状态编排

真正处理命令的逻辑在：

- `crates/control-tower-service/src/main.rs:549-650`

这里可以看出 service 的职责是：

- 接收 IPC 命令。
- 调 `ServiceState` / `MihomoManager`。
- 返回统一的 `IpcResponse`。

总体思路是对的，但有一处行为需要特别注意：

- `IpcCommand::Stop` 在成功停止 Mihomo 后，会设置全局 `SHUTDOWN` 标志。
- 证据位于 `crates/control-tower-service/src/main.rs:566-575`。

这意味着当前 `Stop` 的语义不是“停止 Mihomo”，而是：

1. 停止 Mihomo
2. 顺便让 service 自己也退出

如果后续 CLI / TUI / Web 认为 service 是长期驻留的控制面，这个语义会引发很多问题：

- 停 Mihomo 后，控制面本身也消失。
- 下次状态查询需要重新拉起 service。
- `Stop` 与 `Shutdown` 两个概念被混成一个命令。

这是一个典型的“命令语义不清”问题。

### 2.2.3 主循环模型

主循环在：

- `crates/control-tower-service/src/main.rs:1693-1743`

实现方式是：

- 启动 Unix socket server。
- 每隔 60 秒检查一次 cron 任务。
- 循环调用 `handle_one()`。
- 没有请求时 sleep 10 ms。

代码里也明确写了注释：

- `crates/control-tower-service/src/main.rs:1708-1710`

说明当前实现不是理想的 async I/O 模型，而是偏同步轮询。

从本地工具型服务的角度，这种模型可以接受，但问题是：

- IPC 响应和 cron 检查复用同一主循环。
- 慢请求会拖延其他请求处理。
- 后续如果要增加更多运维动作，主循环会更容易堆积阻塞。

### 2.2.4 对外部系统的调用方式

在订阅更新逻辑中，service 使用 blocking client 下载 profile：

- `crates/control-tower-service/src/main.rs:1746-1816`
- 尤其是 `crates/control-tower-service/src/main.rs:1757-1769`

这与主循环的同步模型叠加后，进一步加重了阻塞特性。

如果后续要修复，不建议第一步就全面 async 化；更现实的做法是：

1. 先把慢操作从主循环剥离。
2. 再统一定义命令执行模型。
3. 最后视需要决定是否迁移到 async。

### 2.2.5 定时任务模型

`scheduler.rs` 位于：

- `crates/control-tower-service/src/scheduler.rs:5-88`

它实际实现的不是 cron expression，而是“每 N 分钟执行一次”的 interval。

例如：

- `Schedule::parse()` 只接受纯数字分钟数：`crates/control-tower-service/src/scheduler.rs:13-31`
- `next_run_seconds()` 直接将分钟换算成秒：`crates/control-tower-service/src/scheduler.rs:33-36`

但 `Profile` 结构体里字段名是 `cron`：

- `crates/control-tower-cli/src/profile.rs:23-25`

这会导致两个问题：

1. 命名与实现不一致。
2. 维护者会误以为支持标准 cron 表达式。

这是文义层面的技术债，虽然不是立刻导致 Bug 的问题，但长期一定会误导后续修改。

---

## 2.3 `control-tower-service-core`

### 2.3.1 暴露的公共 API

`lib.rs` 很薄：

- `crates/control-tower-service-core/src/lib.rs:6-14`

导出的对象有：

- `ServiceState`
- `ServiceStatus`
- `MihomoManager`
- `ServiceConfig`
- `MihomoInstaller`

这说明它想作为 service 的核心能力包存在。

### 2.3.2 状态模型

服务状态集中在：

- `crates/control-tower-service-core/src/state.rs:15-205`

这里最有价值的是 `CircuitBreaker`：

- `crates/control-tower-service-core/src/state.rs:15-112`

它实现了：

- 连续失败计数
- 熔断进入 cooldown
- cooldown 结束后允许重试
- 启动成功后 reset

从后端可靠性视角看，这是当前设计里比较成熟的一块。

但 `ServiceState` 本身仍然是命令式状态对象：

- `set_running`
- `set_error`
- `set_starting`
- `set_stopping`
- `set_not_running`

位置在：

- `crates/control-tower-service-core/src/state.rs:165-205`

这种写法简单，但问题是：

- 状态迁移规则没有集中约束。
- 不同调用方可以自由切状态。
- 后续一旦状态更多，就容易出现非法迁移。

### 2.3.3 `MihomoManager` 职责过胖

`MihomoManager` 位于：

- `crates/control-tower-service-core/src/mihomo.rs:23-316`

它同时负责：

- 查找 Mihomo 二进制
- 自动下载 Mihomo
- 校验可执行权限
- 构造启动命令
- 启动 / 停止子进程
- 复制 `geoip.metadb` / `geosite.db`
- 管理日志输出
- 持有熔断器

其中明显的副作用逻辑包括：

- 自动下载安装：`crates/control-tower-service-core/src/mihomo.rs:59-89`
- 启动与文件复制：`crates/control-tower-service-core/src/mihomo.rs:173-257`

这说明它不是单纯的进程管理器，而是一个混合对象：

- 既像 Supervisor
- 又像 Installer Coordinator
- 还像 Deployment Helper

从后端设计上，这会让后续问题难以拆解：

- 启动失败，到底是下载失败、文件权限问题、Geo 数据缺失，还是子进程问题？
- 单元测试难做，集成测试会被迫覆盖大量基础设施副作用。
- 一旦未来支持别的运行后端，替换成本高。

### 2.3.4 `ServiceConfig` 实现与测试漂移

实现位置：

- `crates/control-tower-service-core/src/config.rs:21-35`

默认值是：

- `socket_path = /tmp/ctsvc.sock`
- `pid_file = /tmp/ctsvc.pid`

但测试却断言：

- `config_dir` 应包含 `clash-verge`
- `socket_path` 应为 `/tmp/verge/clash-verge-service.sock`

位置在：

- `crates/control-tower-service-core/src/config.rs:78-83`

这是非常明确的信号：

- 代码曾经改过路径策略。
- 当前实现与测试没有同步收敛。
- 项目里已经存在“历史设计残影”。

这类问题的危险不在于测试失败本身，而在于会持续误导后续维护者对系统默认路径的理解。

---

## 三、关键数据流与真实控制链路

## 3.1 激活 profile 的链路

代码入口：

- `crates/control-tower-cli/src/profile.rs:198-246`

当前链路为：

1. 从 `profiles.yaml` 找到目标 profile。
2. 读取 profile 文件。
3. 覆盖写入 `config.yaml`。
4. 如果 Mihomo API 不可达，则启动 service。
5. 否则重启 service。

### 判断

这是一个典型的“配置文件发布 + 重载生效”模型。

优点：

- 简单直接。
- 故障排查时只要看 `config.yaml` 和服务状态即可。

问题：

- 运行配置更新不是原子操作。
- 是否需要重启由“API 是否可达”来决定，而不是由“控制面状态”决定。
- 启动 / 重启 decision 放在 CLI，而不是 service。

---

## 3.2 模式切换的链路

代码入口：

- `crates/control-tower-cli/src/service.rs:188-215`

当前链路为：

1. 调 Mihomo 的 `/configs` API 修改 mode。
2. 成功后把 mode 写入 `settings.yaml`。
3. 再尝试同步写回 `config.yaml`。

### 判断

这里与 profile 激活链路不一致。

- profile 激活是：**先写配置，再重启生效**。
- mode 切换是：**先改运行态，再回写配置文件**。

也就是说，系统当前有两种“状态变更策略”：

1. 文件驱动型
2. 运行态驱动型

这是架构上最危险的问题之一，因为它意味着：

- 同一类配置项没有统一真源。
- 故障时很难判断以文件为准，还是以运行态为准。
- Web / TUI / CLI 很容易各自补一套同步逻辑。

---

## 3.3 Profile 自动更新链路

代码入口：

- `crates/control-tower-service/src/main.rs:1746-1816`

当前链路为：

1. 根据 URL 下载订阅内容。
2. 覆盖 profile 文件。
3. 尝试直接修改 `profiles.yaml` 中对应 profile 的 `updated_at`。

注意这里路径来源是：

- `current_exe().parent()` 下的 `profiles.yaml`
- 位置在 `crates/control-tower-service/src/main.rs:1775-1781`

而 CLI 侧 `profiles.yaml` 路径来自：

- `crates/control-tower-cli/src/config.rs:10-23`
- 受 `settings.working_dir` 影响

这再次证明：

- service 与 CLI 不一定在操作同一份配置目录。
- 自动更新链路很可能绕过 CLI 的配置路径约定。

---

## 四、问题清单与修改建议

以下建议分为 3 类：

- **P0：** 影响系统正确性，应优先确认并修复。
- **P1：** 影响可维护性和扩展性，建议尽快治理。
- **P2：** 影响一致性和未来理解成本，可在前两类问题稳定后处理。

---

## 4.1 P0：统一配置目录与配置真源

### 问题

当前至少存在以下配置状态来源：

- `settings.yaml`
- `profiles.yaml`
- `config.yaml`
- Mihomo 运行态 API

并且不同入口对配置目录的理解不一致：

- CLI：`crates/control-tower-cli/src/config.rs:10-23`
- Web API：`crates/control-tower-cli/src/api.rs:44-48`
- service 自动更新：`crates/control-tower-service/src/main.rs:1775-1781`
- core 默认配置：`crates/control-tower-service-core/src/config.rs:21-35`

### 风险

- 不同入口可能读写不同目录。
- 自动更新可能更新了错误的 `profiles.yaml`。
- 运行配置与持久化配置长期漂移。

### 修改建议

1. **定义单一配置根目录解析函数。**
   - 不要在 CLI、Web、service、core 中各写一套 `get_config_dir()`。
   - 抽到公共模块，例如 `service-core::config` 或单独的 `config-paths` 模块。

2. **明确真源。**
   建议采用以下原则：
   - `profiles.yaml`：订阅元数据真源。
   - `config.yaml`：当前激活运行配置快照。
   - `settings.yaml`：本地偏好与运行参数，不保存业务真相。
   - Mihomo API：运行态镜像，不是真源。

3. **统一所有入口的路径解析。**
   以下位置必须改为同一套实现：
   - `crates/control-tower-cli/src/config.rs`
   - `crates/control-tower-cli/src/api.rs`
   - `crates/control-tower-service/src/main.rs`
   - `crates/control-tower-service-core/src/config.rs`

### 建议的验收标准

- `CLI / Web / service` 打印出的配置目录一致。
- `profile add / activate / update / cron` 均操作同一份 `profiles.yaml`。
- 自动更新后，CLI 可立即看到更新时间变化。

---

## 4.2 P0：厘清 `Stop` 与 `Shutdown` 的命令语义

### 问题

`IpcCommand::Stop` 当前会同时：

1. 停 Mihomo
2. 让 service 退出

证据：

- `crates/control-tower-service/src/main.rs:566-575`

### 风险

- 控制面与被控进程耦合过紧。
- 前端或 CLI 很难建立稳定的控制预期。
- 一旦后续有驻留型 TUI / Web 控制面，停 Mihomo 会导致控制面失联。

### 修改建议

1. **拆分命令语义。**
   - `StopMihomo`：只停 Mihomo，不停 service。
   - `ShutdownService`：关闭 service 自身。

2. **保留兼容层时要明确标注。**
   如果短期不想改协议，可先让 `Stop` 只停 Mihomo，再新增 `Shutdown`。

3. **在 CLI 侧同步改调用语义。**
   - `ctctl service stop` 应指向你最终定义的业务语义。

### 建议的验收标准

- 停止 Mihomo 后，仍可查询 service `status`。
- service 可以在 Mihomo 未运行时继续接受 `start` 等命令。

---

## 4.3 P0：统一状态变更策略，避免“先改运行态、再回写文件”

### 问题

当前 mode 切换流程：

- 先调用 Mihomo API
- 再写 `settings.yaml`
- 再尝试写 `config.yaml`

位置：

- `crates/control-tower-cli/src/service.rs:188-215`

### 风险

- 运行态修改成功但文件写回失败。
- 文件写回成功但运行态未真正一致。
- 不同入口未来会复制更多“双写补偿逻辑”。

### 修改建议

建议统一成一种模式，优先推荐：

- **配置文件是真源，service 负责应用到运行态。**

落地方式：

1. CLI / Web 不直接改 Mihomo API。
2. CLI / Web 只向 service 发“业务命令”，例如 `SetMode`。
3. service 内部负责：
   - 更新目标配置文件
   - 调 Mihomo API 或触发 reload
   - 返回最终结果

这样可以把状态一致性问题收口到 service 一层。

### 建议的验收标准

- mode 切换的所有入口只经过 service。
- CLI 不再直接调用 Mihomo `/configs`。
- 失败时可以明确区分“持久化失败”与“运行态应用失败”。

---

## 4.4 P1：将 `MihomoManager` 拆成更清晰的职责单元

### 问题

`MihomoManager` 当前职责过多，见：

- `crates/control-tower-service-core/src/mihomo.rs:23-316`

### 风险

- 测试困难。
- 启动失败根因不清。
- 后续引入新运行后端时替换成本高。

### 修改建议

建议拆成至少 3 个方向：

1. **BinaryLocator / Installer**
   - 查找 Mihomo。
   - 下载并安装二进制。

2. **RuntimeAssetManager**
   - 复制 `geoip.metadb` / `geosite.db`。
   - 校验运行时依赖文件。

3. **ProcessSupervisor**
   - 构造启动命令。
   - 启停子进程。
   - 查询 PID 与运行状态。
   - 管熔断逻辑。

短期如果不想大改，也建议先做最小拆分：

- 把下载安装逻辑从 `MihomoManager` 中抽出。
- 把 Geo 文件同步逻辑抽出。

### 建议的验收标准

- “下载失败”“文件缺失”“启动失败”在日志和错误返回中可明确区分。
- `MihomoManager` 文件体积与职责显著缩小。

---

## 4.5 P1：为状态迁移建立统一入口

### 问题

`ServiceState` 当前提供多种 setter：

- `set_running`
- `set_error`
- `set_starting`
- `set_stopping`
- `set_not_running`

位置：

- `crates/control-tower-service-core/src/state.rs:169-205`

### 风险

- 状态迁移规则分散。
- 未来新增状态时容易出现不合法切换。

### 修改建议

可以逐步收敛为：

- `transition(event)` 模式
- 或至少用更少的显式状态迁移函数，例如：
  - `on_start_requested`
  - `on_start_succeeded`
  - `on_start_failed`
  - `on_stop_requested`
  - `on_stop_succeeded`

这样做的价值不在于“代码更优雅”，而在于：

- 状态变化更容易审计。
- 日志、指标、错误码可统一挂载在事件上。

---

## 4.6 P1：把慢操作从 service 主循环剥离

### 问题

当前主循环和 cron、阻塞下载逻辑耦合。

证据：

- 主循环：`crates/control-tower-service/src/main.rs:1693-1743`
- 阻塞下载：`crates/control-tower-service/src/main.rs:1757-1769`

### 风险

- 下载期间其他命令响应变慢。
- 连接关闭、状态查询等轻量操作也会受影响。

### 修改建议

1. **先不要急着全面 async 化。**
2. 先把 profile 更新任务放到独立工作线程或任务执行器。
3. 主循环只负责接命令、分发任务、查询状态。

短期目标是“主循环不做慢 I/O”，而不是“所有代码都改成 async”。

### 建议的验收标准

- 进行 profile 自动更新时，`status` / `logs` 仍能快速返回。
- 主循环日志中不再出现长时间卡顿。

---

## 4.7 P2：统一“cron”命名与真实实现

### 问题

字段名和用户认知叫 `cron`，实际实现是 interval minutes。

证据：

- `crates/control-tower-cli/src/profile.rs:23-25`
- `crates/control-tower-service/src/scheduler.rs:13-31`

### 修改建议

二选一：

1. **改名。**
   - 字段从 `cron` 改为 `interval_minutes` 或 `update_interval_minutes`。
   - CLI 文案同步修改。

2. **补齐真正的 cron expression 支持。**
   - 如果产品方向确实需要按固定时刻执行。

从当前项目复杂度看，我更建议先走 **改名**，因为这更符合真实能力边界。

---

## 4.8 P2：修复测试与实现漂移

### 问题

`ServiceConfig` 的测试断言与实现不一致。

证据：

- 实现：`crates/control-tower-service-core/src/config.rs:21-35`
- 测试：`crates/control-tower-service-core/src/config.rs:78-83`

### 修改建议

在修复前先做一件事：

- **不要立刻改测试或改实现。先确认当前设计想要什么。**

建议判断顺序：

1. 当前项目实际运行时 socket 应该在哪里。
2. 现有 CLI 和 service 是否都使用同一路径。
3. 最终选定一个统一策略后，再同步修改实现和测试。

否则很容易把“历史残影”修成“新的错误共识”。

---

## 五、建议的后续走读顺序

如果目标是继续深入并指导修复，建议按下面顺序走读。

## 第 1 步：先厘清配置真源与目录解析

优先看这些文件：

- `crates/control-tower-cli/src/config.rs`
- `crates/control-tower-cli/src/settings.rs`
- `crates/control-tower-cli/src/api.rs`
- `crates/control-tower-service/src/main.rs`
- `crates/control-tower-service-core/src/config.rs`

要回答的问题：

1. 当前配置根目录到底以谁为准。
2. 哪些文件是业务真源，哪些只是派生物。
3. CLI / Web / service 是否会读写不同目录。

这是最优先的走读任务，因为它直接影响后续所有修复判断。

---

## 第 2 步：完整追一遍 profile 激活与 mode 切换链路

优先看这些文件：

- `crates/control-tower-cli/src/profile.rs`
- `crates/control-tower-cli/src/service.rs`
- `crates/control-tower-service/src/main.rs`
- `crates/control-tower-service-core/src/mihomo.rs`

要回答的问题：

1. 哪些状态变化是“先改文件”，哪些是“先改运行态”。
2. 哪些业务决策本应在 service，却被放在 CLI。
3. 如果改成 service 收口，最小切口在哪里。

---

## 第 3 步：梳理 service 的命令模型与生命周期

优先看这些位置：

- `crates/control-tower-service/src/main.rs:549-650`
- `crates/control-tower-service/src/main.rs:1693-1743`
- `crates/control-tower-service-core/src/state.rs`

要回答的问题：

1. `Start / Stop / Restart / Status` 的语义是否一致。
2. service 是否应该长期驻留。
3. 状态迁移是否存在非法路径。

---

## 第 4 步：最后再处理 core 的职责拆分

优先看这些文件：

- `crates/control-tower-service-core/src/mihomo.rs`
- `crates/control-tower-service-core/src/installer.rs`
- `crates/control-tower-service-core/src/state.rs`

要回答的问题：

1. 哪些逻辑属于进程管理。
2. 哪些逻辑属于安装部署。
3. 哪些逻辑应该变成纯状态模型或纯配置模型。

---

## 六、推荐的修复推进策略

如果准备真正改代码，建议按以下顺序推进，而不是一次性做大重构。

### 第一阶段：只做正确性修复

目标：先避免状态漂移和行为语义错误。

建议修改项：

1. 统一配置目录解析。
2. 修正 `Stop` 命令语义。
3. 明确 mode / profile 的状态变更真源。
4. 修复 `ServiceConfig` 测试与实现漂移。

### 第二阶段：做边界收口

目标：把运行态控制尽量收回 service。

建议修改项：

1. CLI 不再直接改 Mihomo API。
2. service 扩展 IPC 命令承接 mode / proxy / connections 管理。
3. Web 与 CLI 尽量共享同一套应用服务接口。

### 第三阶段：做结构治理

目标：为后续功能扩展降低成本。

建议修改项：

1. 拆分 `MihomoManager`。
2. 收敛 `ServiceState` 的状态迁移方式。
3. 调整 scheduler 的命名或能力边界。
4. 视需要决定是否迁移到真正的 async 模型。

---

## 七、结论

当前 `crates/` 目录的实现已经具备可运行的基本骨架，但从后端工程质量看，存在 3 个核心问题：

1. **配置与状态的真源不统一。**
2. **入口层与应用层的职责边界不够硬。**
3. **core 层承担了过多基础设施副作用。**

如果只从“先把代码跑起来”的角度看，这套实现已经够用；但如果目标是：

- 让 TUI / CLI / Web 共享统一行为，
- 让订阅更新、模式切换、进程控制的语义一致，
- 为后续修 Bug 和继续扩功能打基础，

那么建议优先处理本文列出的 P0 问题，尤其是：

- 配置目录统一
- 命令语义统一
- 状态变更策略统一

这三件事一旦理顺，后续走读和修复会容易很多。

---

## 附：本轮走读直接引用的关键代码位置

- CLI 入口：`crates/control-tower-cli/src/main.rs:22-79`
- CLI 配置：`crates/control-tower-cli/src/settings.rs:81-151`
- CLI 配置目录：`crates/control-tower-cli/src/config.rs:10-23`
- Profile 管理：`crates/control-tower-cli/src/profile.rs:40-246`
- CLI Service 调用：`crates/control-tower-cli/src/service.rs:27-215`
- Web API 配置目录：`crates/control-tower-cli/src/api.rs:44-48`
- IPC 协议：`crates/control-tower-service/src/main.rs:18-49`
- 命令处理：`crates/control-tower-service/src/main.rs:549-650`
- Service 主循环：`crates/control-tower-service/src/main.rs:1693-1743`
- Profile 自动更新：`crates/control-tower-service/src/main.rs:1746-1816`
- Scheduler：`crates/control-tower-service/src/scheduler.rs:5-88`
- Core 导出：`crates/control-tower-service-core/src/lib.rs:6-14`
- State 与熔断器：`crates/control-tower-service-core/src/state.rs:15-205`
- Mihomo 管理：`crates/control-tower-service-core/src/mihomo.rs:23-316`
- Core 配置：`crates/control-tower-service-core/src/config.rs:21-83`
