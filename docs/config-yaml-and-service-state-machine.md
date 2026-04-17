# `config.yaml` 变更链路与 service 协议 / 状态机走读（第三轮）

## 文档目的

本文同时覆盖两条深入走读主线：

- **A：`config.yaml` 的完整变更链路**
- **B：`control-tower-service` 的 IPC 协议与状态机**

前两轮文档已经确认：

1. 配置路径解析不统一。
2. CLI / Web / service 存在多条平行控制路径。
3. 同名能力在不同入口上的语义不一致。

本文继续向下钻，重点不是重复描述现象，而是回答两个更接近“能不能开始修”的问题：

1. `config.yaml` 目前到底被哪些地方修改，这些修改会不会互相覆盖。
2. 现有 service 协议与状态设计，能不能承接后续统一应用层。

---

## 一、先给结论

### 结论 1：`config.yaml` 当前不是“中心配置”，而是“多个模块共享写入的活动文件”

从实现看，`config.yaml` 被如下能力共同读写：

- profile 激活
- profile 删除
- mode 切换
- rule 增删导入
- service 启动 / 重启

但这些能力没有统一写入入口，也没有统一同步策略。

因此它现在更像是：

- 一个所有人都能改的共享活动文件
- 而不是一个由单一应用层管理的配置真源

### 结论 2：`config.yaml` 写入存在明确的“整文件覆盖风险”

当前至少有两类写法：

1. **整文件替换**
   - 例如 profile 激活时直接把 profile 文件内容覆盖成 `config.yaml`
2. **读全文件 → 改局部字段 → 写全文件**
   - 例如 rule 和 mode 修改

这两类写法叠加后，意味着：

- mode / rule 刚改完磁盘内容，下一次 profile 激活就可能整文件抹掉
- profile 激活后的配置，又可能被后续字段级改写部分覆盖

### 结论 3：当前 service 的 IPC 协议能跑，但还不足以承接“统一应用层”

原因不是协议完全缺失，而是：

- 现有命令集合偏“内核控制”而非“业务命令”
- 返回结果只有 `code/message/data`，没有细粒度错误语义
- 状态机在 `core` 中已有雏形，但 `service` 主体并没有真正接入
- `Stop` 的真实语义混合了“停 Mihomo”和“停 service”

### 结论 4：如果现在开始修，最优先的不是“重构成 async service”，而是“先统一写入入口与命令语义”

也就是说，当前最值得做的不是大改技术栈，而是先完成：

1. `config.yaml` 写入收口
2. `Stop / Restart / Status` 语义收口
3. 把 mode / proxy / connections 等运行态操作逐步收回 IPC

---

## 二、A 线：`config.yaml` 的完整变更链路

## 2.1 `config.yaml` 的使用位置

从当前代码看，`config.yaml` 至少承担以下角色：

1. 当前激活的 Mihomo 配置文件
2. rule 管理的持久化落点
3. mode 修改的磁盘同步目标
4. service 启动 / 重启时传入 Mihomo 的配置路径

相关定位：

- core 路径方法：`crates/control-tower-service-core/src/config.rs:60-63`
- CLI start 发送 `config_path`：`crates/control-tower-cli/src/service.rs:285-365`
- service 执行启动：`crates/control-tower-service/src/main.rs:550-559`
- service restart 默认路径：`crates/control-tower-service/src/main.rs:591-599`

### 判断

`config.yaml` 已经是“运行配置事实文件”，但不是单一来源生成，而是多个模块共同维护。

---

## 2.2 链路一：profile 激活会整文件覆盖 `config.yaml`

关键实现：

- `crates/control-tower-cli/src/profile.rs:198-246`

### 行为

当执行 `ctctl profile activate <id>` 时：

1. 找到对应 profile 文件。
2. 读取 profile 文件全文。
3. 直接 `std::fs::write(config.yaml, content)`。
4. 如果服务没跑就启动，否则重启。

### 这意味着什么

这是标准的**整文件替换**。

也就是说，profile 激活并不关心当前 `config.yaml` 上是否已经有：

- 额外 rule
- mode 改动
- 其他运行期持久化字段

它会直接以 profile 文件内容为准全部覆盖。

### 风险

如果用户执行过：

- `ctctl rule add`
- `ctctl mode set`

然后再执行 `profile activate`，那么此前在 `config.yaml` 上做过的本地变更，有很大概率被直接冲掉。

### 原子性

当前写法是直接 `fs::write`，不是：

- 写临时文件
- 校验
- rename 原子替换

因此在异常中断时，存在文件处于半写入或不完整状态的潜在风险。

---

## 2.3 链路二：profile 删除会删除 `config.yaml`

关键实现：

- `crates/control-tower-cli/src/profile.rs:143-155`

### 行为

删除 profile 后，会继续：

1. 删除 `config.yaml`
2. 调用 `stop_service()`

### 问题

这是把“订阅元数据变更”和“运行配置清理 / 运行态停止”硬耦合到一起。

更关键的是，执行顺序是：

- 先删 `config.yaml`
- 后停 service

如果停服务失败，就会出现：

- Mihomo 可能还在跑
- 但磁盘上的 `config.yaml` 已经没了

这是一个典型的持久化状态与运行态短暂分离问题。

---

## 2.4 链路三：rule 操作是“改局部字段，但重写整文件”

关键实现：

- `crates/control-tower-cli/src/rule.rs:163-200`
- `crates/control-tower-cli/src/rule.rs:214-247`
- `crates/control-tower-cli/src/rule.rs:255-326`

### 行为模式

无论是：

- `rule add`
- `rule remove`
- `rule import`

都遵循同一个模式：

1. 读取整个 `config.yaml`
2. 反序列化成 YAML Value
3. 修改 `rules` 字段
4. 再把整个 YAML 序列化回去
5. 整文件重写

### 逻辑特点

从业务上看，它是“局部字段修改”；
但从物理写盘上看，它依然是“整文件覆盖”。

### 风险

#### 风险 1：并发丢更新

如果未来出现两个入口几乎同时修改 `config.yaml`：

- 一个在改 rule
- 一个在改 mode

那么后写入的一方可能覆盖前一方的更新。

#### 风险 2：与 profile 激活互相踩踏

如果 `rule add` 刚修改完 `config.yaml`，紧接着执行 `profile activate`，那么整文件覆盖会把 rule 改动抹掉。

#### 风险 3：与运行态不同步

rule 修改后当前实现只是提示：

- `Use 'ctctl service restart' to apply changes.`

也就是说：

- 磁盘状态已更新
- 运行态并未同步

这是一个明确存在的“持久化已变，内存 / 进程未变”窗口。

---

## 2.5 链路四：mode 切换是“先改运行态，再尽力追写 `config.yaml`”

关键实现：

- `crates/control-tower-cli/src/service.rs:188-240`

### 当前步骤

执行 `ctctl mode set` 时：

1. 对 Mihomo 调 `PATCH /configs`
2. 成功后写 `settings.yaml`
3. 再调用 `update_config_mode()` 修改 `config.yaml`

而 `update_config_mode()` 的实现位于：

- `crates/control-tower-cli/src/service.rs:217-240`

逻辑同样是：

1. 读全文件
2. 改 `mode`
3. 写全文件

### 这条链路的特殊性

它和 profile / rule 最大区别在于：

- profile / rule 更偏“先改磁盘，再等重启生效”
- mode 是“先改运行态，再追写磁盘”

### 风险

#### 风险 1：运行态成功，磁盘失败

代码里如果 `update_config_mode()` 失败，只会打 warning，不会回滚运行态。

于是会出现：

- Mihomo 当前 mode 已切换
- `settings.yaml` 也可能已更新
- `config.yaml` 仍然是旧值

#### 风险 2：Web API 行为又不同

Web `set_mode` 位于：

- `crates/control-tower-cli/src/api.rs:317-347`

它只会：

- 调 Mihomo `/configs`

不会写：

- `settings.yaml`
- `config.yaml`

这意味着同样是 mode 变更：

- CLI 改动具备部分持久化
- Web 改动更像纯运行态临时修改

#### 风险 3：service 重启后恢复逻辑不统一

service 启动后会从 `settings.yaml` 恢复 mode：

- `crates/control-tower-service/src/main.rs:416-423`
- `crates/control-tower-service/src/main.rs:450-489`

所以 mode 的实际持久化真源更像是 `settings.yaml`，而不是 `config.yaml`。

这说明：

- `config.yaml` 被用来保存 mode
- 但 mode 真正的恢复逻辑却来自 `settings.yaml`

属于典型的双写但真源不一致。

---

## 2.6 链路五：service restart 默认取固定路径 `config.yaml`

关键实现：

- `crates/control-tower-service/src/main.rs:591-599`

### 行为

`Restart` 命令不是“按上次 `Start` 使用过的路径重启”，而是直接取：

- `current_exe().parent()/config.yaml`

### 风险

这会造成两个问题：

1. `Restart` 语义依赖固定目录，不依赖当前 effective config path。
2. 一旦 CLI 的 `working_dir` 与 service 的可执行目录不同，重启就可能用错配置文件。

### 判断

这说明 `config.yaml` 不只是内容层面有问题，连“使用哪一份 `config.yaml`”在路径层面也还没有统一。

---

## 2.7 `config.yaml` 当前最根本的问题

可以用一句话概括：

> `config.yaml` 当前是多入口共享修改的活动文件，但系统没有提供统一的写入与同步模型。

具体表现为：

- profile 会整文件覆盖它
- rule 会局部修改但整文件重写它
- mode 会先改运行态再尽力追写它
- service restart 依赖它，但路径又未统一

因此，如果不先收口 `config.yaml` 写入，后续任何“修 mode / 修 rule / 修 profile”的工作，都很容易陷入互相打架。

---

## 三、B 线：service IPC 协议与状态机走读

## 3.1 当前 IPC 协议长什么样

协议定义在：

- `crates/control-tower-service/src/main.rs:18-49`

命令包括：

- `Start { config_path }`
- `Stop`
- `Restart`
- `Status`
- `Logs { lines }`
- `GetProxies`
- `GetConnections`
- `CloseConnection { id }`
- `ReloadCron`

传输层在：

- `crates/control-tower-service/src/main.rs:772-809`

具体形式是：

- Unix socket `/tmp/ctsvc.sock`
- JSON 单行报文
- 响应统一 `{code,message,data}`

### 优点

- 简单，容易调试
- 本地控制场景足够轻量

### 局限

- 无协议版本
- 无请求 ID
- 无能力协商
- 无结构化错误码
- 无事件流 / 订阅机制

对于当前项目规模，这不是立刻要命的问题；但如果要承接统一应用层，结构化错误语义很快会成为瓶颈。

---

## 3.2 各命令的真实语义

## `Start`

实现位置：

- `crates/control-tower-service/src/main.rs:383-426`
- `crates/control-tower-service/src/main.rs:552-565`

### 真实行为

1. 用调用方传入的 `config_path` 启动 Mihomo
2. 如果已运行，则直接返回成功
3. 启动成功后尝试从 `settings.yaml` 恢复：
   - 已选 proxy
   - mode

### 观察

`Start` 不只是启动内核，还包含“恢复运行态偏好”的副作用。

这说明它更像“启动并恢复会话状态”，而不是单纯的内核启动命令。

---

## `Stop`

实现位置：

- `crates/control-tower-service/src/main.rs:510-518`
- `crates/control-tower-service/src/main.rs:566-580`
- `crates/control-tower-service/src/main.rs:1708-1742`

### 真实行为

1. 停止 Mihomo
2. 设置 `SHUTDOWN = true`
3. service 主循环退出
4. 退出前再次确保 Mihomo 停止

### 结论

`Stop` 实际语义不是“停止 Mihomo”，而是：

- **停止整个 service，并顺带终止 Mihomo**

这是当前协议里最不合理的一点，因为它把：

- 控制面生命周期
- 被控进程生命周期

绑在了同一个命令上。

---

## `Restart`

实现位置：

- `crates/control-tower-service/src/main.rs:582-609`

### 真实行为

1. 先 stop
2. 固定用 `exe_dir/config.yaml` 再 start

### 问题

这不是“基于当前有效配置重启”，而是“基于服务自己猜出来的默认配置文件重启”。

如果前一次 `Start` 用的是别的路径，这里不会继承。

---

## `Status`

实现位置：

- `crates/control-tower-service/src/main.rs:520-541`
- `crates/control-tower-service/src/main.rs:610-614`

### 返回内容

只返回：

- `running`
- `pid`
- `uptime_secs`

### 问题

它看不到：

- service 自身是否健康
- 是否处于熔断态
- 最近错误是什么
- 当前 effective config path 是什么
- cron 是否已装载

也就是说，它提供的是“进程是否活着”的最小视图，而不是“控制面状态”。

---

## `Logs`

实现位置：

- `crates/control-tower-service/src/main.rs:192-206`
- `crates/control-tower-service/src/main.rs:615-620`

### 真实行为

它返回的是 service 内存中的环形日志，不是：

- `ctsvc.log`
- `mihomo.log`

这意味着调用方如果以为拿到的是内核日志，会产生误判。

---

## `GetProxies` / `GetConnections` / `CloseConnection`

实现位置：

- `crates/control-tower-service/src/main.rs:313-381`
- `crates/control-tower-service/src/main.rs:621-642`

### 真实行为

这些命令本质上是 service 代为调用：

- `http://127.0.0.1:9090/proxies`
- `http://127.0.0.1:9090/connections`

### 意义

这很重要，因为说明 service 已经具备承接运行态查询和控制的能力。

换句话说，CLI / Web 现在直接打 Mihomo API，不是因为 service 做不到，而是因为当前架构没有强制收口。

---

## `ReloadCron`

实现位置：

- `crates/control-tower-service/src/main.rs:208-270`
- `crates/control-tower-service/src/main.rs:644-647`

### 真实行为

它只是重新扫描 `profiles.yaml` 的 cron 定义并装载，不会立即执行任务，也不影响 Mihomo 运行态。

---

## 3.3 service 自身生命周期与 Mihomo 生命周期的关系

service 主循环在：

- `crates/control-tower-service/src/main.rs:1693-1743`

### 当前关系

- service 启动时不会自动拉起 Mihomo
- service 退出时会停止 Mihomo
- `Stop` 会同时结束两者

### 判断

这是一种“部分耦合”关系：

- 不是完全独立
- 也不是明确的一主一从控制模型

如果后续目标是让 service 成为长期驻留控制面，那么更合理的关系应该是：

- service 生命周期独立于 Mihomo
- Mihomo 只是 service 管理的一个 runtime component

---

## 3.4 状态机现在到底存在不存在

### core 中其实有显式状态机雏形

定义在：

- `crates/control-tower-service-core/src/state.rs:114-205`

状态包括：

- `NotRunning`
- `Starting`
- `Running`
- `Stopping`
- `CircuitBroken(u64)`
- `Error(String)`

并且提供了显式 setter：

- `set_starting`
- `set_running`
- `set_stopping`
- `set_not_running`
- `set_error`

### 但 service 主体并没有真正使用这套状态

service 自己真正持有的是另一个 `ServiceState`：

- `crates/control-tower-service/src/main.rs:167-189`

包含：

- `MihomoManager`
- `start_time`
- `log_buffer`
- `cron_jobs`

然后通过 `manager.is_running()` + `pid()` 推导状态：

- `crates/control-tower-service/src/main.rs:520-541`

### 结论

当前项目处于一种“状态机设计存在，但主流程没有接入”的中间状态。

这会带来几个问题：

1. 上层看不到 `Starting / Stopping / Error / CircuitBroken`
2. 熔断器虽然存在于 `MihomoManager`，但 `Status` 不暴露
3. service 内部的真实状态迁移过程没有统一模型承载

---

## 3.5 错误边界现在在哪里

### 协议层错误

- JSON 解析失败：`parse_message()`
- 位置：`crates/control-tower-service/src/main.rs:148-158`

### IPC 层错误

- socket accept / read / write 失败
- 位置：`crates/control-tower-service/src/main.rs:687-809`

### 运行态错误

- Clash API 调用失败
- Mihomo 启动失败
- 下载 Mihomo 失败
- 配置文件不存在
- Geo 数据复制失败

关键位置：

- `crates/control-tower-service/src/main.rs:313-381`
- `crates/control-tower-service-core/src/mihomo.rs:173-290`

### 现在的问题

尽管错误很多层，但对上层返回时基本都被压成：

- `code = -1`
- `message = String`

这会导致：

- CLI / Web 无法做可靠分类处理
- 后续如果需要用户态恢复策略，会很难分辨失败类型

---

## 四、对后续修复最有指导意义的判断

## 4.1 `config.yaml` 应该被收口为“单一写入通道”

### 不是说它要成为唯一真源

从当前业务形态看，更现实的设计是：

- `profiles.yaml`：订阅元数据真源
- `settings.yaml`：本地环境参数与偏好真源
- `config.yaml`：当前活动运行配置快照

### 但 `config.yaml` 必须有统一写入层

建议至少引入一个集中写入对象，例如：

- `ActiveConfigStore`
- 或 `ConfigRepository`

统一负责：

- 读取当前配置
- 应用 profile 激活
- 应用 mode 改动
- 应用 rule 改动
- 原子写回
- 生成备份 / 校验失败回滚

如果这一步不做，后续任何业务修复都会继续互相踩。

---

## 4.2 service 协议应该从“内核命令”升级为“应用命令”

当前命令更像：

- start kernel
- stop kernel
- read kernel status
- proxy query passthrough

如果要承接统一应用层，建议新增或重构成更贴近业务的命令，例如：

- `ActivateProfile { id }`
- `SetMode { mode }`
- `AddRule { rule }`
- `RemoveRule { index }`
- `ImportRules { ... }`
- `GetRuntimeStatus`
- `GetServiceStatus`
- `ShutdownService`

### 价值

这样 CLI / Web / TUI 都只调用同一套业务命令，而不是各自去拼：

- 改文件
- 调 Mihomo API
- 再补一层持久化

---

## 4.3 `Stop` 必须拆语义

建议拆成：

- `StopMihomo`
- `ShutdownService`

因为当前 `Stop` 的混合语义会直接阻碍 service 作为常驻控制面的定位。

---

## 4.4 `Restart` 必须绑定“当前有效配置”而不是“默认目录猜测”

建议未来改成以下二选一：

1. `RestartUsingCurrentConfig`
2. `Restart { config_path }`

而不是现在这种：

- 内部固定猜测 `exe_dir/config.yaml`

---

## 4.5 必须把 core 的显式状态机真正接入 service 主流程

当前 `core::state::ServiceStatus` 已经有不错的状态表达能力：

- `Starting`
- `Stopping`
- `CircuitBroken`
- `Error`

但主流程没有使用它。

如果后续准备做：

- 更稳定的 status API
- 更清晰的错误展示
- 自动恢复 / 熔断观测

那么这套状态机必须真正成为 service 的状态来源，而不是停留在 core 里。

---

## 五、建议的实际改造顺序

## 阶段 1：先做最小正确性收口

1. 统一配置路径解析
2. 让 `Restart` 使用统一路径来源
3. 拆分 `Stop` 与 `Shutdown`
4. 收口 `config.yaml` 写入入口

## 阶段 2：收口业务命令到 service

1. `SetMode` 走 IPC
2. `GetProxies / GetConnections / CloseConnection` 统一通过 service
3. Web 复用 CLI 的应用层，而不是直打 Mihomo API

## 阶段 3：状态机与错误语义升级

1. 接入 core 显式状态机
2. `Status` 分裂为更清晰的状态接口
3. 增加结构化错误码
4. 必要时再考虑 async 化

---

## 六、结论

到第三轮走读，问题已经非常聚焦：

### `config.yaml` 的问题本质是“共享写入、缺乏收口”

它不是单纯的某个函数写错，而是多个模块都在以不同语义修改同一活动文件。

### service 的问题本质是“已有基础，但协议与状态模型还没升级到应用层”

它已经能管理 Mihomo，也已经能代理部分运行态操作；但当前命令集、状态表达和错误边界，还不足以成为统一应用服务层。

因此，后续修复如果想真正见效，优先级应该是：

1. `config.yaml` 写入收口
2. `Stop / Restart / Status` 语义收口
3. mode / proxy / connections 等控制路径收回 IPC
4. 显式状态机接入 service 主流程

这 4 件事做完，后面再修 profile、rule、Web API 行为不一致，会容易很多。

---

## 附：本轮直接引用的关键代码位置

- profile 激活覆盖 `config.yaml`：`crates/control-tower-cli/src/profile.rs:198-246`
- profile 删除清理 `config.yaml`：`crates/control-tower-cli/src/profile.rs:143-155`
- mode 修改与回写：`crates/control-tower-cli/src/service.rs:188-240`
- rule add/remove/import：`crates/control-tower-cli/src/rule.rs:163-326`
- start service 传 `config_path`：`crates/control-tower-cli/src/service.rs:285-365`
- service restart 默认 `config.yaml`：`crates/control-tower-service/src/main.rs:582-609`
- IPC 协议：`crates/control-tower-service/src/main.rs:18-49`
- IPC 传输：`crates/control-tower-service/src/main.rs:772-809`
- service 状态对象：`crates/control-tower-service/src/main.rs:167-189`
- service 运行态代理能力：`crates/control-tower-service/src/main.rs:313-381`
- service 启动恢复 proxy / mode：`crates/control-tower-service/src/main.rs:406-508`
- service 主循环：`crates/control-tower-service/src/main.rs:1693-1743`
- core 显式状态机：`crates/control-tower-service-core/src/state.rs:114-205`
- Mihomo 启停与熔断：`crates/control-tower-service-core/src/mihomo.rs:173-316`
- core 配置路径：`crates/control-tower-service-core/src/config.rs:60-63`
