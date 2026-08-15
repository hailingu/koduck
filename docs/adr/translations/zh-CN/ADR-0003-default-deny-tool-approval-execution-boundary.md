# ADR-0003：默认拒绝的工具审批与执行边界（中文翻译）

> [!IMPORTANT]
> 本文件是
> [`docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`](../../ADR-0003-default-deny-tool-approval-execution-boundary.md)
> 的非权威中文翻译，不是第二份 ADR，也不拥有独立的状态、审批或记录身份。
> 若中英文存在差异，以 `docs/adr/INDEX.md` 索引的英文 ADR 为准。状态、
> 子任务、验收检查、证据和链接必须与英文版同步更新。

## 元数据 [Required]

- **决策状态**：Accepted
- **实施状态**：In Progress
- **日期**：2026-08-12
- **作者**：@codex
- **决策负责人**：@linhai
- **所需审批人**：@linhai
- **记录范围**：Project
- **审批人 [Conditionally Required — Decision Status 为或曾为 `Accepted`]**：@linhai
- **审批时间 [Conditionally Required — Decision Status 为或曾为 `Accepted`]**：2026-08-12T17:56:05+08:00
- **审批证据 [Conditionally Required — Decision Status 为或曾为 `Accepted`]**：Approve
- **拒绝执行人 [Conditionally Required — Decision Status 为 `Rejected`]**：N/A — Decision Status 为 `Accepted`，而非 `Rejected`
- **拒绝时间 [Conditionally Required — Decision Status 为 `Rejected`]**：N/A — Decision Status 为 `Accepted`，而非 `Rejected`
- **拒绝证据 [Conditionally Required — Decision Status 为 `Rejected`]**：N/A — Decision Status 为 `Accepted`，而非 `Rejected`
- **退役执行人 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`；记录未退役
- **退役时间 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`；记录未退役
- **退役证据 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`；记录未退役
- **退役原因 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`；记录未退役
- **阻塞前状态 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `In Progress`，而非 `Blocked`
- **阻塞项与证据 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `In Progress`，而非 `Blocked`
- **阻塞项负责人 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `In Progress`，而非 `Blocked`
- **阻塞退出或复查条件 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `In Progress`，而非 `Blocked`
- **相关资料 [Optional]**：[Koduck Trello 卡片 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87)；`docs/adr/ADR-0001-provider-neutral-turn-kernel.md`
- **架构来源 [Conditionally Required — 产品需求]**：`docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-2
- **取代 [Conditionally Required — 本 ADR 替换其他 ADR]**：None
- **被取代 [Conditionally Required — 本 ADR 被替换]**：None

## 要求级别图例 [Required]

- **`[Required]`**：章节或字段始终适用，必须保留并提供完整、可验证的内容。只有模板明确允许空结果时，才可使用 `None — <原因>`，不得留空。
- **`[Conditionally Required — <触发条件>]`**：触发条件成立时必须完成；不成立时保留 `N/A — <原因>`，除非模板明确要求删除或作为未来生命周期说明保留。未评估触发条件即为内容不完整。
- **`[Optional]`**：删除后不影响审批、实施、完成或验证；如保留则必须准确完整，且不能替代必填证据。

`[Required]` 章节内未单独标注的字段均为必填。

## 背景与问题陈述 [Required]

ADD-0001 CAND-1 已完成，提供了本任务所需的已认证、Provider 中立的 Turn
内核、先持久化后发布的持久历史，以及带 Fencing 的前台 Lease Generation。
该内核有意不包含工具调用或 MCP 执行路径。CAND-2 现在是第二个按依赖排序
的开发候选项：它必须加入工具执行能力，同时不允许模型输出、传输元数据、
过期的 Turn Owner 或用户可见的审批投影授予权限。

本 ADR 把这一个 ADD 候选项转换为一个可独立评审的实施切片。它拥有 C-5
策略、规范 D-6 Approval Request、单次尝试的 D-7 Execution Attempt、
已认证审批传输通道，以及只能通过隔离 Executor 访问 Tool 或 MCP 能力的
Adapter 的详细契约。该切片建立安全边界，但不启用任何生产特权能力：初始
生产描述符 Allowlist 为空，而确定性 Fixture 会完整演练允许、审批、拒绝、
Fencing、超时、取消和输出处理契约。

## 范围 [Required]

范围内：

- 自有 Action、Descriptor、Effect、Permission Profile、策略决策、
  Approval Request、Execution Attempt 和审计事件类型。
- 一个位于每个原生 Tool 和 MCP 调用之下的 C-5 应用边界，与 CAND-1
  Provider 中立的 Turn Runner 集成。
- 规范 PostgreSQL D-6/D-7 状态、精确尝试审批绑定、不可变 D-3 投影 Item，
  以及当前 C-6 前台租约校验。
- 一条已认证 REST 决策路由，以及用于待决审批和执行终态的持久 SSE 投影；
  不包含可视化 UI。
- Consumer-owned 的 Tool/MCP 描述符 Adapter，以及一个向外部隔离 Tool
  服务或 Worker 发送有界 Envelope 的 Executor Client Port。
- 使用隔离 Executor 测试 Harness 和合成 Tool/MCP 清单的确定性生产边界
  集成测试。

范围外：

- 可复用的会话级或 Turn 级审批授权、审批缓存，或会改变 Permission
  Profile 的审批。
- 启用任何生产特权 Tool/MCP 描述符，或向 `koduck-ai` 增加宽泛的主机
  文件系统、进程、凭据或任意网络访问。
- 从 AI 内核或 Presentation Adapter 直接派生 MCP stdio 进程，或直接
  访问 MCP 端点。
- 扩展发现、优先级、来源快照、Skill、Plugin 和仓库指令加载，仍属
  CAND-4。
- 后台 Multitask 执行、Fork/Checkpoint 语义、语义 Memory、部署、晋级
  和 UI 设计。

## 张力、约束与开放问题 [Required]

### 已识别张力 [Conditionally Required — 存在相互竞争的目标或权衡]

| ID | 张力 | 影响 | 决策 |
| --- | --- | --- | --- |
| TN-1 | 工具实用性与最小权限相互竞争。 | 宽松的默认策略会让不可信的模型或 MCP 内容造成主机或外部副作用。 | 未知描述符和效果一律拒绝；生产描述符 Allowlist 从空开始；每个启用的描述符必须匹配固定的效果与 Permission Profile 规则。 |
| TN-2 | 审批响应速度与持久、多实例权限相互竞争。 | 进程内 Waiter 很快，但会在崩溃和跨实例时丢失或重复权限。 | PostgreSQL D-6/D-7 状态为权威；本地通知只是优化，每次状态迁移都使用条件持久写入。 |
| TN-3 | 取消响应速度与如实的效果报告相互竞争。 | 在 Executor 可能已开始效果后宣称取消，会掩盖部分影响。 | Executor 报告效果为 `not_started`、`started` 或 `unknown`；派发后的 Fencing 或取消会根据该观测状态产生精确的 cancelled 或 failed 终态。 |
| TN-4 | 重试提高可用性，但可能重复特权效果。 | 重放一个已开始或状态不明的尝试可能重复非幂等动作。 | 仅当 Executor 证明先前尝试从未开始效果时，才允许最多一次自动重试；重试获得新的 D-7 身份，并重新进行策略/审批评估。 |

### 约束 [Required]

- ADD-0001 保持 `Current`，CAND-1 保持 `Complete`，本 ADR 保持与 CAND-2
  的精确双向链接。
- C-5 是策略、D-6 和 D-7 的唯一权威；C-1 传输决策，C-2 负责编排，C-6
  负责持久化，D-3 Item 是非权威视图。
- Provider、Tool、MCP、HTTP、PostgreSQL 和 Executor 的 Wire Type 不得
  进入领域策略类型。
- C-2 和 C-1 绝不执行直接的主机执行或接触 MCP 端点；实际效果经
  Consumer-owned Executor Port 进入隔离的外部边界。
- 签名身份和 `ai.tool.approve` Scope 来自 C-7。转发的 Header 或请求体
  不能捏造审批人身份或 Scope。
- 一个 Turn 解析出一个不可变 Permission Profile。审批只授权一个精确的
  D-7，绝不改变或扩大该 Profile。
- 每个外部可见的审批或执行投影必须先完成持久 Append 再发布，并引用
  规范的 D-6/D-7 版本。
- 所有维护型 Rust 变更遵循公共与 Rust 开发标准；本 ADR 不批准任何
  工程例外。

### 开放问题 [Conditionally Required — 存在或起草期间解决过重大问题]

| ID | 问题 | Owner | 截止日期 | 状态 | 结论与证据 |
| --- | --- | --- | --- | --- | --- |
| Q-1 | 本切片启用哪些生产 Tool/MCP 能力？ | @linhai | 2026-08-12 | Resolved | 无。初始生产描述符 Allowlist 为空；合成描述符仅存在于测试中。启用真实特权能力超出 CAND-2 范围，需要针对其精确范围的 Accepted 决策。 |
| Q-2 | 原生 Tool 和 MCP 效果在哪里执行？ | @linhai | 2026-08-12 | Resolved | 通过一个 Consumer-owned 的 Executor Client Port 到达外部隔离的 Tool 服务或 Worker。本切片中 `koduck-ai` 没有直接的文件系统/进程/MCP 传输实现。 |
| Q-3 | 什么身份可以决定一个待决审批？ | @linhai | 2026-08-12 | Resolved | 经 C-7 验证、具有 `ai.tool.approve` 的同一租户 Principal；规范请求同时固定所属 Thread、Turn、动作摘要和 D-7 身份。 |

## 决策驱动因素 [Required]

1. **不可绕过的权限**：每个原生 Tool 和 MCP 路径必须经过同一个默认拒绝
   策略与 Executor 边界。
2. **精确持久授权**：一次审批必须恰好授权一个不可变尝试，并在多实例
   竞争下存活且不扩大范围。
3. **带 Fencing 的所有权**：过期的 CAND-1 前台 Owner 既不得派发效果，
   也不得提交其结果。
4. **如实的失败语义**：超时、取消、Executor 失败、部分/未知效果状态
   和不可信输出必须保持可区分且可审计。
5. **有界实施切片**：任务必须在一个可评审的 Pull Request 内扩展现有
   Rust 服务，不启用生产特权，也不吞并 CAND-3/CAND-4。

## 考虑过的方案 [Required]

### 方案 A：Consumer-owned 的 C-5 状态机，使用持久存储与隔离 Executor Port

现有 `koduck-ai` Crate 拥有模型中立的策略与生命周期类型，通过其 AI 自有
PostgreSQL 边界持久化 D-6/D-7，并把每个效果委托给一个外部 Executor
Port。Tool 和 MCP Adapter 可以翻译描述符和结果，但不能绕过 C-5 派发。

优点：

- 保持向内依赖，并给每条执行路径一个统一权威。
- 通过持久的条件迁移支持精确尝试审批、租约 Fencing 和多实例竞争。
- 把主机和 MCP 执行保持在 AI 服务信任边界之外。

缺点：

- 为每次 Tool 调用增加持久迁移和跨边界延迟。
- 即使生产能力 Allowlist 仍为空，也需要一个 Executor 集成契约。

### 方案 B：把策略和审批放进每个 Tool/MCP Adapter

每个原生 Tool 和 MCP Adapter 各自评估自己的 Allowlist、请求审批，并通过
其偏好的传输执行。

优点：

- 每个 Adapter 可以独立实现。
- 初期需要的共享应用类型更少。

缺点：

- 策略、身份、重试和审计行为可能漂移或被绕过。
- 新 Adapter 会成为新的安全权威，且必须重新实现 Fencing。
- 与 ADD-0001 的 C-5 所有权矛盾。

### 方案 C：把审批投影或模型确认当作权限

Turn 历史包含一个待决审批 Item 和后续表示同意的内容；Runner 使用该
Item 继续执行。

优点：

- 复用现有历史流，新增持久化最少。

缺点：

- 不可信内容或过期投影可能授予权限。
- 无法原子地绑定身份、精确参数、Scope 和单个尝试。
- 违反 D-6 规范权威不变量。

## 决策 [Required]

**选择方案**：方案 A — Consumer-owned 的 C-5 状态机，使用持久存储与
隔离 Executor Port。

**理由**：方案 A 是唯一能在不把执行能力放进模型、HTTP、Tool 或 MCP
Wire Type 的情况下集中权限的方案。持久的条件 D-6/D-7 迁移让审批与
CAND-1 租户所有权和 Lease Generation 对齐，而外部 Executor Port 使隔离
成为真实的失败与信任边界。以空的生产 Allowlist 起步，在不静默授权新
能力的前提下证明了该机制。

### 后果 [Required]

正面：

- 原生 Tool 和 MCP 请求拥有同一个默认拒绝、精确尝试策略。
- 审批身份、动作范围、Lease Generation、尝试和终态证据相互关联且持久。
- 新 Adapter 不能仅凭声明效果或在其输出中返回指令来授予权限。
- CAND-4 之后可以提供带来源的描述符，而无需改变 C-5。

负面：

- 工具执行增加 PostgreSQL 和隔离 Executor 的往返。
- 首次实施不提供任何已启用的生产 Tool，因此用户可见的工具实用性只能
  通过后续 Accepted 的能力决策到来。
- 外部效果开始后的 Fencing 可能产生效果为 `started` 或 `unknown` 的
  失败尝试；禁止自动重试。

缓解措施：

- 保持 D-6/D-7 写入小、有索引、条件化，并由 PostgreSQL 集成测试覆盖。
- 发布明确的 Typed 拒绝/失败结果，而不是隐藏空清单或 Executor 不可用。
- 记录效果状态，并要求对不明确的外部结果进行运维/人工对账；绝不自动
  重试。

### 详细设计 [Required]

#### 效果与权限清单

| Effect ID | 含义 | 基线策略 |
| --- | --- | --- |
| `read_data` | 从显式配置的能力读取有界非机密数据。 | 仅当描述符 ID/版本和目标在生效的 Permission Profile 内时，才允许不经审批执行。 |
| `external_write` | 在规范 AI 历史之外创建、更新或删除状态。 | 需要精确尝试审批；本 ADR 不启用任何生产描述符。 |
| `filesystem_write` | 通过隔离 Executor 修改文件。 | 需要精确尝试审批；本 ADR 不启用任何生产描述符。 |
| `process_execute` | 通过隔离 Executor 启动进程或向其发信号。 | 需要精确尝试审批；本 ADR 不启用任何生产描述符。 |
| `network_egress` | 访问固定 Executor 端点之外的目标。 | 需要精确尝试审批和目标受限的 Profile；本 ADR 不启用任何生产描述符。 |
| `credential_use` | 在 Executor 边界使用被引用的凭据。 | 需要精确尝试审批；凭据值绝不进入 D-3/D-6/D-7 或日志；本 ADR 不启用任何生产描述符。 |
| `unknown` | 缺失、不支持、过期或冲突的效果元数据。 | 拒绝，不派发 Executor，也无审批路径。 |

在后续 Accepted 记录指明实际描述符、目标限制、所属服务和验证方式之前，
生产清单保持为空。测试为每个策略类别使用合成描述符。描述符仅当其 ID 为
1–128 个 ASCII 字节、版本固定、JSON Schema 有效、声明的效果为清单中的
一个值且序列化输入不超过 65,536 字节时才有效。

#### 规范记录与状态机

D-6 包含 `approval_id`、租户/主体/Thread/Turn 身份、D-7 尝试 ID、描述符
ID/版本、效果、精确动作摘要、有界展示摘要、Permission Profile ID/版本、
Lease Generation、请求/过期时间戳、决策身份、状态和单调递增的记录版本。
动作摘要覆盖规范的描述符 ID/版本、目标、参数、效果、Profile、Turn、
Lease Generation 和 D-7 身份。任何变化都会创建一个新尝试，并在需要时
创建一个新审批。

D-6 的迁移为 `requested -> accepted | declined | cancelled | expired`。
从 `requested` 出发只有一个条件迁移能成功。过期时间为 Turn Deadline 与
请求创建后五分钟两者中较早者。已认证决策路由为
`POST /api/v1/ai/approvals/{approval_id}/decisions`，接收一个仅包含
`decision: accepted | declined | cancelled` 的精确 JSON 对象；缺失身份
返回 `401`，未知、跨租户、跨 Thread 或未授权的审批身份返回不可区分的
`404` 且不改变任何记录。重复的相同决策返回现有终态投影；冲突的决策
返回 `409 approval-already-resolved`。

D-7 包含精确的动作 Envelope 与摘要、效果状态、Lease Generation、时间戳、
状态、有界结果元数据和审计关联信息。其迁移为 `prepared -> running ->
succeeded | failed | timed_out | cancelled`；被 declined、cancelled 或
expired 的 D-6 会将其仍处于 prepared 的 D-7 迁移为 `cancelled` 且
`effect_state=not_started`。一个 Turn 同一时间最多运行一个 D-7，最多创建
16 个尝试。一个 Tool 动作最多运行 30 秒，最多产生 1,048,576 个序列化输出
字节，且最多有一次自动的效果前重试。超过上限的 Executor 结果被丢弃并
记录为 `failed/output_limit_exceeded`。每次初始执行和每次重试都消耗 16
个 D-7 尝试槽位中的一个。若分配重试会超出该 Turn 预算，则不创建重试
记录或派发，当前动作以 `failed/attempt_limit` 终止。

#### 策略、Fencing、执行与重试

Turn Runner 把经过验证的模型 Tool 调用转换为自有 Action，查找已配置的
描述符快照，并调用 C-5。C-5 在准备 D-7 之前校验描述符、精确输入、不可变
Profile、租户、Turn 和当前 Lease Generation。`unknown` 或超出 Profile 的
动作被拒绝，不创建 D-6 也不派发 Executor。Profile 内的 `read_data` 动作
可以不经 D-6 直接派发。每个特权效果都会创建 D-6 并等待其规范终态。

C-5 在条件 `prepared -> running` 迁移和 Executor 派发之前立即再次校验
同一个当前 Lease Generation。结果提交时再次进行条件校验。派发前的
Fencing 保持 `effect_state=not_started` 并取消 D-7。派发后的 Fencing 不
向模型提交任何 Tool 结果：Executor 确认为 `not_started` 的尝试为
`cancelled`；`started` 或 `unknown` 为
`failed/owner_fenced_after_dispatch`。

只有 Executor 响应证明 `effect_state=not_started` 时才允许一次自动重试。
重试获得新的 D-7 身份，重新运行描述符、Profile 和租约策略，并且每个
需要审批的效果都需要新的 D-6。它计入 Turn 的 16 次尝试预算中的第二次
尝试。`started` 或 `unknown` 的尝试绝不自动重试。

已认证的 Turn 中断会取消处于 requested 的 D-6 和 prepared 的 D-7，或为
running 的 D-7 发送一个有界取消请求。Executor 以 `not_started` 或
`started` 确认时，产生带有该报告状态的 `cancelled`。若 Executor 在 30 秒
动作 Deadline 前未确认，D-7 为 `timed_out` 且 `effect_state=unknown`。
该 Turn 仍受 CAND-1 单一持久终态仲裁约束。

#### Adapter、投影与审计契约

原生 Tool 和 MCP Adapter 产生相同的自有描述符和 Action。MCP 的名称、
Schema、内容、错误和结果保持不可信；MCP 传输或服务端声明不能改变其
已配置的效果/Profile。`koduck-ai` 只把有界的自有 D-7 Envelope 发送到
配置的 Executor 端点。本切片中它既不派生 MCP stdio 进程，也不直接接触
MCP 服务端。

D-3 增加仅追加的 `approval_status`、`tool_call` 和 `tool_result` Payload。
每个投影携带其规范的 D-6/D-7 身份和版本，并且只在持久 Append 之后发布。
它不能被读作授权。Tool 输出是不透明的不可信结果，只有在当前 Generation
提交成功后才提供给模型；它绝不会被解析为 Permission Profile、描述符、
指令来源或审批。

审计证据关联租户假名、Thread、Turn、描述符版本、Profile 版本、动作摘要、
Lease Generation、策略决策、D-6 版本、D-7 迁移、Executor 效果状态、时序、
字节数和稳定的终态代码。一条序列化审计记录必须不超过 16,384 字节。它
排除凭据值以及原始动作参数或结果内容；哈希、字节数、稳定代码和有界
展示摘要提供关联。

#### 规范契约条款

- **TC-01 — 单一权威**：每个原生 Tool 和 MCP 调用必须进入 C-5；
  `koduck-ai` 不得包含任何绕过 Executor Port 的直接主机进程、文件系统
  效果或 MCP 服务端执行路径。
- **TC-02 — 默认拒绝**：缺失、过期、禁用、不兼容、冲突或未知的
  描述符/效果元数据必须产生 Typed 拒绝、零个 D-6 和零次 Executor 派发。
- **TC-03 — 不可变 Profile**：一个 Turn 必须保留一个 Permission Profile
  ID/版本；模型、描述符、Tool/MCP 输出和审批不得扩大它。
- **TC-04 — 精确审批**：一个 accepted 的 D-6 必须恰好授权其绑定的租户、
  Thread、Turn、Lease Generation、描述符版本、效果、动作摘要、Profile
  版本和 D-7 身份，不授权任何其他尝试。
- **TC-05 — 已认证决策**：只有经 C-7 验证、具有 `ai.tool.approve` 的
  同一租户 Principal 可以决议 requested 的 D-6；无效的所有权或 Scope
  不得改变任何状态，也不得暴露审批的存在。
- **TC-06 — 投影非权威**：D-3 审批/工具投影必须是规范 D-6/D-7 版本的
  仅追加视图，不得授权或重新派发执行。
- **TC-07 — 租约 Fencing**：C-5 必须在准备时、派发前一刻和结果提交时
  校验当前前台 Lease Generation；被 Fence 的 Owner 不得向模型提交任何
  Tool 结果。
- **TC-08 — 重试安全**：自动重试必须最多发生一次，且只能在
  `effect_state=not_started` 之后；它必须使用新的 D-7 和全新策略，需要
  审批时还须使用新的 D-6，且每次重试必须消耗 Turn 的 16 个 D-7 尝试
  槽位之一。
- **TC-09 — 有界执行**：一个 Turn 必须最多有一个 running 的 D-7 和 16
  次总尝试；一个动作必须在 30 秒和 1,048,576 个序列化输出字节处停止，
  并给出精确的终态/代码。
- **TC-10 — 取消真实性**：中断必须在不派发的情况下关闭 requested 审批，
  或发出一个有界的 Executor 取消；Deadline 时未获确认的取消必须为
  `timed_out/effect_state=unknown`。
- **TC-11 — 不可信结果**：Tool/MCP 输出不得改变描述符、权限、审批、
  身份或执行路由，并且只有在当前 Generation 持久结果提交成功后才能
  到达模型。
- **TC-12 — 持久并发**：相互竞争的审批决策、派发方、终态结果和对账方
  必须使用条件的规范迁移；恰好一个迁移胜出，且同一个 D-7 尝试不会
  派发两次。
- **TC-13 — 禁用恢复**：未晋级的派发方必须通过移除其运行时启用项来
  禁用；失败必须让 Tool/MCP 不可用，且不得调用前身或直接 Fallback
  路径。
- **TC-14 — 审计最小化**：每个策略/审批/执行终态必须产生最多 16,384
  字节的序列化关联元数据，不包含凭据值或原始动作参数与结果内容。

## 实施计划 [Required]

**完整任务结果**：新的 Koduck AI Turn 生命周期中，每个原生 Tool 和 MCP
调用都经过一个默认拒绝的 C-5 策略和隔离的、单次尝试的 D-7 Executor
边界；需要审批的工作使用一个规范的精确动作 D-6，并产生有界、带
Fencing、可取消、可审计的终态证据，而不启用生产特权能力。

**主要实施边界**：`koduck-ai` 中的 C-5 策略、审批与执行应用边界；相邻的
C-1/C-3/C-6 和 Runtime 变更仅限于该边界所需的传输、Tool 调用输入、
持久化和 Executor 接线。

允许的子任务状态：`Not Started`、`In Progress`、`Blocked`、`Complete` 或
`N/A — <具体原因>`。

| ID | 目标或交付物 | 包含范围 | 状态 | 实际实施证据 |
| --- | --- | --- | --- | --- |
| T-1 | 实现自有 Tool/MCP Action、Descriptor、Effect、Profile、C-5 策略、D-6/D-7 状态机、边界、重试、取消和租约 Fencing 行为。 | `koduck-ai` 领域/应用模块、Consumer-owned Port、Runner 集成、带意图说明的公开文档、聚焦的单元/契约测试。 | In Progress | Test-first 的 Source 现已定义自有描述符/动作/效果/Profile、默认拒绝决策、精确目标作用域的 Permission Profile ID/版本绑定，以及 Adapter 校验的 JSON/object-schema 输入——独立的 65,536 字节动作输入上限和描述符 Schema 上限在解析前强制执行，任意精度十进制文本被保留。每个不可信动作 Envelope 字段在哈希或 D-7 分配前都有界：描述符 ID 和版本上限为 128 字节，精确目标上限为 256 字节，并拒绝 ASCII/控制字符；Permission Profile Allowlist 按同一动作边界校验每个条目，使超大的配置条目无法扩大 Envelope。Permission Profile ID 和版本通过 Profile 构造函数与精确动作绑定共用的一个共享校验器限制在 128 字节并拒绝 ASCII/控制字符，使 Profile 身份在哈希或保留进 D-6/D-7 状态之前即有界。JSON Adapter 和领域 Schema 构造函数都在策略评估前拒绝重复属性。非权威的 `ToolPolicy` 评估不再能 Seal 绑定：Crate 自有的 Sealing 服务通过 Crate 自有的配置 Port 解析描述符/Profile 快照，重新检查精确的 Profile 身份/版本，并在 D-6/D-7 创建前写入私有的审批要求。审批要求 Setter 受单一调用点防护，因此没有 Tool/MCP Adapter 能自我授权特权绑定。被拒绝的绑定两者都不分配，已授权的 `read_data` 不经 D-6 直接派发。条件/幂等的审批决议需要 Crate 自有的 C-7 Authorizer 和决策服务，同时独立强制同租户和同 Thread 所有权。对需要审批的动作，C-7 所有权与作用域预校验在任何 D-7 分配之前运行——包括 D-6 窗口已过期的调用，它在分配后取消路径之前完成授权——因此未授权调用不会留下已准备尝试，重复的无作用域请求无法耗尽 Turn 的 16 槽位预算（TC-05）。Source 和聚焦测试定义了一个显式注入的强 Turn 权威根；其构造函数为 Crate 自有且非全局，因此公开调用方无法构造第二个根来重置重复 D-7 拒绝、单运行尝试仲裁或 16 槽位预算。该根在临时句柄丢失期间强保留进程内状态；在 T-3 能证明规范 Turn 终态并防止预算复活之前，回收有意不可用。所有 Executor 成功与失败路径在终态提交前立即校验当前租约。受防护的迁移、精确的 `concurrent_attempt`、较早 Deadline 到期和规范 SHA-256 动作摘要均有聚焦证据。一个 C-5 Tool 执行 Driver 现已编排 authorize、prepare、approve 与 execute，并带有恰好一次已证明的效果前重试（TC-08）：它仅在已提交的 Executor `Failed{effect_state=NotStarted}` 终态时重试（绝不在成功或取消时重试），分配新的 D-7 身份、重新运行描述符/Profile 策略，并为需要审批的效果创建新的 D-6；被 Declined、Cancelled 或过期的 D-6（包括在 D-6 到期后到达的决策）通过受防护的 Coordinator 路径把仍处于 Prepared 的 D-7 关闭为 `cancelled/not_started`，且不派发；耗尽 16 槽位预算的重试返回 `failed/attempt_limit`；并且受控时钟在每次 D-6 创建和派发时重新读取（审批决策也携带其实际决策时间），使延迟审批无法把 D-6 窗口或 D-7 启动时间钉死在原调用时间；而重试准备阶段被 Fence 的所有者返回错误，不送达任何已提交的旧终态。聚焦的内部 Fixture 覆盖重试逻辑（`cargo test -p koduck-ai --lib cand_2_retry_tests`：效果前未开始的成功、started/unknown、至多一次、新 D-6、预算耗尽的 `failed/attempt_limit`、对账、declined/cancelled 取消、成功/取消不重试以及迟到决策的到期）；这些仅为逻辑层覆盖——AC-9 通过公共运行时/Runner 入口的端到端重试验证仍是 T-2 交付物，待该公共边界存在后由黑盒 `tests/cand_2_retry.rs` 验证。取消与超时逻辑现已实现：Executor Port 在一个不透明的 Permit 之后携带一次有界取消，在 30 秒动作 Deadline 时报告已确认的效果状态或没有任何确认；Coordinator 在取消一个 running 的 D-7 前校验当前租约，以 Executor 报告的状态提交 `cancelled`，在取消未获确认时提交 `timed_out/unknown`，并在每次 Executor 响应后（包括有界取消响应后）重新读取受控时钟，使在 30 秒动作期限之后到达的取消确认提交 `timed_out/unknown` 而非 `cancelled`，并使观测完成时间达到 30 秒的动作以如实的 effect-state 证据提交 `timed_out`，而非成功或失败的结果；中断句柄通过共享进程权威解析 Turn 已登记的 prepared 或 running D-7，并经同一受防护的条件提交路径关闭它，被 Fence 的所有者会收到对账要求而运行中尝试保持已登记，已取消 D-7 的迟到 Executor 响应被拒绝且不交付模型。`DisabledExecutor` 将取消边界报告为不可用，因此 C-5 会保留运行中的 D-7 供对账，而不会在动作期限前伪造超时；禁用的运行时仍不暴露任何效果路径。聚焦的内部 Fixture 覆盖取消与超时契约（`cargo test -p koduck-ai --lib cand_2_cancellation_tests`：prepared 中断零派发且零取消调用、已确认 not_started/started 终态、未确认 `timed_out/unknown`、被 Fence 所有者对账并保留运行中尝试、两侧精确的 30 秒 Deadline 边界、取消后迟到结果拒绝、非 running 的 Typed 拒绝、未知 Turn 的无操作，以及 Claim 后或派发后租约不可用时保留运行中尝试的终态保留、使中断对从未派发或已执行的效果发出零次 Executor 取消）；与重试 Fixture 一样这些仅为逻辑层覆盖——AC-8 通过已认证传输的端到端取消验证仍是 T-2 交付物。与重试 Fixture 一样这些仅为逻辑层覆盖——AC-8 通过已认证传输的端到端取消验证仍是 T-2 交付物。T-1 尚未完成；其包含范围仍要求 Runner 集成。公共 Runner 侧 C-5 入口现已存在于当前未提交任务修订：`koduck-ai/src/application/tool_boundary.rs` 现将整个 C-5 边界面收敛为 crate 内部：`ToolExecutionRuntimeRoot::issue` 为 `pub(crate)` 且运行时装配（`runtime::tool_execution_root`）是其唯一生产调用点，Assembly 与 Boundary 仅以 crate 内部句柄分发，刻意不设进程全局根与公共签发路径，因此任何调用方都无法铸造第二份权威目录（TC-09/TC-12；crate 内部回归证明同一装配的同级 Boundary 与同一根上第二个 Assembly 的 Boundary 均被拒绝 `concurrent_attempt` 与 `attempt_limit` 且自身零派发；架构断言禁止全局根、要求注入根构造函数并固定唯一的运行时签发点）。该装配应用 Crate 自有的配置支撑 Sealing 服务，以及一个要求 `ai.tool.approve` 作用域的 Crate 自有 C-7 作用域 Authorizer——该作用域现建模为封闭的 `ApprovalScopes` 能力，只能在 crate 内部由已认证信任 Adapter 构造，且只能经 `with_approval_scopes(ApprovalScopes)` 附加，因此任何外部调用方都无法铸造审批作用域——生产中仅由 T-2 交付的运行时 C-7 网关验证作用域通道填充，因此无作用域的主体永远无法决议 D-6；调用方只能提供已校验的 `ToolConfigurationSnapshot` 值和 Consumer-owned 的 Executor/租约/提交方 Port，且不接受任何调用方构造的权威存储。C-5 Driver 仅在 `ApprovalDecisionService::validate_resolver` 确认租户、Thread 和 `ai.tool.approve` 作用域之后才调用决策提供方，因此未授权主体的决策回调从未被调用、也观察不到 D-6（TC-05 回归断言回调计数为 0）；`LeaseValidator` Port 现返回类型化的 `LeaseCheck`，其 `Unavailable` 结果——包括校验器 panic 导致共享租约锁中毒——以类型化的 `LeaseUnavailable` 对账传播且零派发，而不是复用可能不完整的校验器状态或伪装成 Fence（TC-07 回归断言类型化的 `ExecutionPreparationError::LeaseUnavailable` 结果；该类型化结果即机器可读诊断，结构化日志接收器需要本切片授权范围之外的日志依赖，随 T-2 运行时可观测性接线落地）。Driver 另外在策略评估和任何 D-7 分配前拒绝租户与已认证主体不一致的调用——包括免审批的 `read_data` 路径——返回精确的 `TenantMismatch` 代码、零 D-6 创建和零派发（跨租户回归）。完整的 AC-10 虚拟时钟边界表以 crate 内部 `koduck-ai/tests/internal/cand_2_limits.rs` Harness 运行：两条审批到期分支均精确到期（Turn Deadline 较晚时 4:59.999/5:00.000，两分钟 Turn Deadline 时 1:59.999/2:00.000，过期用例零派发）；无作用域主体收到 `NotAuthorized` 且零派发；执行恰在 30.000 秒提交 `timed_out`、29.999 秒成功；第 16 次尝试完成而第 17 槽位与耗尽预算的重试分配返回精确的 `attempt_limit` 结果/代码；65,536 字节动作输入可执行而 65,537 字节在解析前返回 `input_limit`；1,048,576 字节输出提交而 1,048,577 字节被丢弃为 `failed/output_limit_exceeded` 且不携带任何 Payload；同一 Turn 的第二个并发动作在恰好一次运行中派发下被拒绝 `concurrent_attempt`。T-1 保持 `In Progress`，因为生产 `TurnRunner`/运行时链路尚未调用该入口，且在 T-2 传输入口点存在之前无法推导 AC-1 的入口委托计数；完成 T-1 需要在既有 T-1 范围内让 Runner 链路消费该边界，或走审批失效流程修订范围。Provider Tool-call 翻译、D-3 投影、HTTP/SSE 审批传输、Executor Client 和运行时配置是尚未开始的 T-2 范围；规范 PostgreSQL 权威与安全回收仍属 T-3 范围。 |
| T-2 | 实现已认证审批与投影传输，以及采用空生产描述符 Allowlist 的 Tool/MCP 和隔离 Executor Adapter。 | REST 决策路由、SSE/D-3 Payload、Provider Tool 调用翻译、Tool/MCP 描述符 Adapter、Executor Client 与运行时配置；无直接主机/MCP 执行。 | In Progress | 新增了可感知重复、Fail-closed 的 JSON-Schema 翻译（反序列化前 65,536 字节 Schema 上限）、由不透明的仅 Coordinator 派发 Permit 保护的 Consumer-owned 隔离 Executor Port，以及一个增量响应构建器——它在缓冲超过 1,048,576 字节前拒绝输出，溢出后无法完成。条件终态提交 Port 区分获胜写入、已存在的规范终态、冲突终态、Fencing 和不可用。现有终态只能通过一个校验非零规范版本和输出上限的类型重建，保留精确的 D-7 绑定，并在该绑定不同时被拒绝用于对账；因此 Coordinator 只返回匹配的有界规范获胜者，绝不返回失败的本地输出。被拒绝的规范派发声明返回非终态的 `ExecutionPending::DispatchRejected` 路径，同时保留已准备或已存在的规范 D-7 状态，因此不会被误认为已持久提交的 Tool 输出；已 Seal 的 Turn 的被拒声明携带独立的中断代码而非审批不匹配诊断，因此调用方不会走上审批失败的恢复路径。Crate 内部生命周期方法使用唯一的 `claim_dispatch`、`mirror_terminal` 和 `allocate_attempt` 命名，架构测试扫描完整生产 Source 图，强制其一个 Coordinator 声明调用点、两个条件提交调用点和一个校验租约的准备方分配调用点，因此没有 D-7 能在缺少 TC-07 当前 Generation 租约检查的情况下被分配。C-7 已验证决策 Setter 同样受单一调用点防护，因此审批传输不能在没有 ApprovalDecisionService 和 ApprovalAuthorizer 的情况下应用调用方提供的决策。`DisabledExecutor` 是唯一的生产 `IsolatedExecutor` 实现，但生产运行时接线仍未完成；架构测试扫描完整 Source 图和 Crate Manifest 以查找直接/遗留执行路径。C-7 审批作用域信任边缘现已存在：按仓库 Owner 于 2026-08-14 在当前任务中的指示（延续 ADR-0001 已验收的 CAND-1 网关验证上下文模型，并明确未选择服务内 JWT/JWKS 验证），运行时 `trust_context` 边界把来自可信 `x-koduck-approval-scopes` 上下文头的网关验证审批作用域经 crate 内部 `ApprovalScopes::from_validated` 密封进 `TrustContext`——至多 16 个 Token，每个至多 128 个 ASCII 字节（`[A-Za-z0-9._:-]`），头缺失即无作用域，而存在但不可读、空 Token、含空白（Token 按原样校验——首尾空白被拒绝而非归一化，因此 `" ai.tool.approve"` 绝不会变成特权作用域）、超长、禁用字符或超数的值使整个身份无效（`401`），而非被静默降级，因为网关绝不发出畸形上下文；16 个 Token 上限现在在复制 Token 之前强制执行，因此超数头在拒绝前至多分配 16 个 Token。`from_validated` 的 dead-code 豁免已移除，`koduck-ai/docs/contracts/cand-2-tool-approval-v1.md` 记录了该通道，且该头在 `koduck-ai/docs/runtime-configuration.md` 中的可强制网关剥离/重发交接现已显式覆盖：网关必须移除调用方提供的值，并且只从其已验证签名 Claims 授予的作用域设置该头，因为运行时不对该头做独立的签名 Claims 验证，转发的调用方值即构成审批作用域注入（TC-05）；无法强制该规则的部署不得暴露审批决策路由。聚焦的运行时测试覆盖作用域密封、缺失、包括 obs-text 字节与未归一化首尾空白在内的畸形拒绝、租户独立性和身份门控（每次修复前均观察到 RED：该头被忽略且畸形值被静默降级为无作用域；带前导空格的 Token 被 trim 成特权作用域）。所配置的网关验证签名 Claims 并负责在信任边界剥离客户端转发的值；koduck-ai 只密封该边界已验证的内容。已认证审批决策路由现已在框架中立边界存在：`ApprovalDecisionRoute`（`koduck-ai/src/application/approval_route.rs`）从网关验证的信任上下文派生密封 `ApproverId`——无作用域或空白主体决议不了任何内容，并观察到不可区分的 `NotFound` 且零存储调用（TC-05）——并把条件持久结果映射为 `Resolved` / `Conflict` / `NotFound` / `Unavailable`，租户作用域由存储键本身强制；`ApprovalDecisionAdapter`（`koduck-ai/src/adapters/http/approvals.rs`）分发 `POST /api/v1/ai/approvals/{approval_id}/decisions`，只接受仅含 `decision: accepted | declined | cancelled` 的精确 JSON 对象，把缺失身份映射为带 `WWW-Authenticate: Bearer` 的 `401`、未知/跨租户/无作用域身份映射为不可区分的 `404`、相同重放映射为同一终态投影、冲突决策映射为 `409 approval-already-resolved`、存储不可用映射为 `503`。Crate 内部 `koduck-ai/tests/internal/cand_2_http.rs` Harness 通过具有相同条件语义的内存规范 double 验证完整契约；AC-6 为 `Pass`。该路由的 Axum 运行时接线与生产存储装配仍是开放的 T-2 传输工作；SSE/D-3 投影与 Provider Tool 调用翻译随后。 |
| T-3 | 持久化规范 D-6/D-7/审计元数据，并证明多实例、Fencing、生产边界和 Fail-closed 行为。 | 幂等 PostgreSQL Migration/Adapter 操作、集成 Harness、竞争/故障/上限测试、契约副本和运行时文档。 | In Progress | 首个增量已在当前未提交任务修订上交付：幂等 Additive Migration `koduck-ai/migrations/0002_cand_2_policy_execution.sql` 创建规范 `tool_approvals` D-6 表，作为部分 Schema 增量——它持久化当前领域记录携带的全部可持久字段（租户/Thread/Turn/attempt 身份、租约 Generation、描述符 ID/版本、效果、hex 动作摘要、Permission Profile ID/版本、请求/到期时间戳、状态、决策、审批人、决策时间和单调递增记录版本），其状态/决策 CHECK 约束额外拒绝任何审批人空白或缺失的终态行，并带待决查找索引；规范 D-6 契约中的已认证请求者主体身份与有界展示摘要均为仍开放字段，留待拥有调用方身份与展示内容的 T-2 审批传输/投影增量落地。Consumer-owned 的 `ApprovalRecordStore` Port（`koduck-ai/src/application/approval_store.rs`）把条件持久结果类型化为 `Won` / `ExistingTerminal` / `NotFound`（TC-12），`SqlxApprovalRecordStore`（`koduck-ai/src/adapters/execution/postgres.rs`）按既有生产 pool/runtime 模式与 2 秒尝试 Deadline 实现它：一条条件 `requested -> terminal` UPDATE（`WHERE status='requested' AND expires_at_millis > decided_at`）恰好提交一个决策并递增版本；失败者读取已提交的规范终态；到期时或到期后的决策不提交任何决策并条件迁移记录为 `expired`；跨租户与未知身份返回 `NotFound` 且零暴露。`insert_requested` 具备重放安全（TC-12）：以 `ON CONFLICT DO NOTHING` 插入并在冲突时对照已提交规范行校验全部不可变字段，对完全相同的丢失确认重放报告携带该行当前规范投影的 `ApprovalInsertResolution::Existing{status, decision, version}`（即使另一实例已决议或使记录过期），对绑定不再匹配的重放报告类型化的 `ApprovalStoreError::IdentityConflict`，因此实际已提交的 2 秒超时始终可对账，而不会表现为永久 `Unavailable`。`resolve_decision` 接收密封的领域 `ApproverId` 能力而非原始字符串：其构造为 crate 内部的 `ApproverId::from_authenticated(&TrustContext)`，仅对已验证密封作用域携带 `ai.tool.approve` 的非空已认证主体成功（`TOOL_APPROVAL_SCOPE` 现由领域定义并由应用策略模块重导出），因此任何外部调用方都无法铸造审批权威或绕过 C-5 决策服务决议 D-6（TC-05）；新增架构回归 `approver_identity_cannot_be_minted_outside_the_crate` 固定“无公有构造器”与已认证派生，持久路径保持非空审批人不变量并以 Schema CHECK 作为纵深防御。`ApprovalRequest` 增加持久化用 `requested_at_millis` 字段/Getter，`ExactActionBinding` 增加 `turn_id`/`lease_generation` 访问器。Crate 内部 Harness `koduck-ai/tests/internal/cand_2_postgres.rs`（以 `KODUCK_AI_TEST_DATABASE_URL` 环境门控）针对一次性生产 PostgreSQL 证明：Migration 连续两次幂等应用、首次插入报告 `Inserted` 而同一记录的丢失确认重放报告 `Existing`、不可变字段不同的诱饵规范行报告 `IdentityConflict`、首个决策以版本 2 获胜而相同重放与冲突决策均观察已提交终态、32 路并发竞争决策恰好产生 1 个获胜者和 31 个相同的 `ExistingTerminal` 结果、到期边界恰在 Deadline 提交 `expired/None` 而窗口内决策仍获胜、另一实例决议或使记录过期后的插入重放返回该终态的规范投影（`Existing{status, decision, version}`）而非 requested 版本 1、且 Schema 拒绝每一个非法终态元组——空白或纯空白审批人、缺少决策时间戳的已决议终态、不早于到期的决策时间戳以及 requested 记录上的决策时间戳（开发中观察到 RED：初始状态 CHECK 拒绝无决策的 `expired` 终态，表现为 `Unavailable`）。AC-12 保持 `Not Started`，因为其命名 Harness 与 D-7 派发声明/终态提交分支尚未实现；D-7/审计表、其余迁移操作和该存储的运行时接线是 T-3 的开放范围。 |

**当前 T-1 证据补充：**C-5
中断现在使用单独提供的 `AttemptCancellationService`，因此认证取消可在派发
Coordinator 阻塞时到达其 Executor 取消客户端。每次持久化 D-7 终态写入前，共享
权威都会原子保留已登记的 `prepared` 或 `running` 状态；派发或第二次取消不能在
该迁移进行时使用过期的镜像尝试。running 取消会在发送外部取消请求前取得该保留，
因此并发中断无法发送第二次取消。中断视图是一次权威锁内快照：任一存活 D-7
正在提交终态时，整个 Turn 都会先返回 Typed 对账，而不会部分关闭另一可见尝试。
同一目录锁会在释放该快照前记录中断墓碑，因此无论已知 Turn 还是此前未知的 Turn，
都不能与后续 D-7 分配发生竞争。
外部取消请求已经发出后，即使发生 Fencing 或持久写入失败也会保留该保留直到对账，
而不会重新开放第二次取消。requested D-6 的取消会报告自身
`requested -> cancelled` 获胜，或报告规范 D-6 已解决；两种情况都会继续条件关闭
关联的 prepared D-7。聚焦测试证明阻塞派发时的可达性、派发与过期 prepared
取消的竞争，以及 D-6 已接受但尚未派发时的处理。独立服务的运行时组装仍属 T-2
工作，未提升 AC-8 状态。条件提交 `Conflict` 同样视为规范 D-7 终态已获胜的
证据，因此其保留会持续到对账完成，而不会重新开放派发或取消。C-5 时间戳在跨越
任一 Executor 边界前会被化为相对的剩余动作预算；Executor 因而使用自己的单调
时钟来强制该预算，而非使用已持久化或受控的绝对时间戳。若 prepared 取消输给并发
的 `prepared -> running` 迁移，C-5 会刷新同一个已登记 D-7 并发送其一次有界的
running 取消，而不是遗留未处理的运行中效果。若派发前已无剩余动作预算，C-5 会以
零 Executor 调用提交 `timed_out/not_started`。若 D-7 已声明为 running 后该派发前
终态提交不可用，C-5 会保留其对账保留，使后续中断无法向一个从未实际派发的
Executor 发送取消。
`DisabledExecutor` 现在将取消边界报告为不可用，因此 C-5 会保留运行中的 D-7
供对账，而不会在动作期限前伪造 `timed_out/unknown`。

**受影响路径**：`Cargo.lock`；`koduck-ai/Cargo.toml`；
`koduck-ai/migrations/0002_cand_2_policy_execution.sql`；
`koduck-ai/src/domain/**`；`koduck-ai/src/application/**`；
`koduck-ai/src/adapters/http/**`；`koduck-ai/src/adapters/provider/**`；
新增 `koduck-ai/src/adapters/execution/**` 和
`koduck-ai/src/adapters/tool/**`；`koduck-ai/src/adapters/history/**`；
`koduck-ai/src/runtime/**`；`koduck-ai/src/lib.rs`；
`koduck-ai/docs/contracts/cand-2-tool-approval-v1.md`；
`koduck-ai/docs/runtime-configuration.md`；聚焦的 Crate 自有
`koduck-ai/tests/internal/cand_2_*.rs` 权威 Fixture；以及
`koduck-ai/tests/**` 契约/集成测试。受此切片影响的现有受治理 Source
Marker 必须在仍适用的此前 ADR 之外同时引用本 ADR。

**迁移与回滚策略 [Conditionally Required — 本变更改变现有行为]**：在
运行时启用前应用 additive、幂等的 D-6/D-7 Migration。生产描述符
Allowlist 保持为空，因此在后续 Accepted 能力记录之前，现有 CAND-1
无工具路径仍是唯一可执行的 Turn 路径。任何契约、Fencing、竞争、
Executor 隔离或 PostgreSQL 检查失败时停止。晋级前，回退或禁用新的
派发方/配置，并将 additive 表保留为未使用的审计 Schema，或仅通过单独
治理的兼容 Migration 移除；此时 Tool/MCP 请求以不可用方式 Fail Closed。
不存在前身、直接执行或 MCP Fallback。已验证 Artifact 晋级后，任何
Artifact 回滚都需要 Accepted OCR，且只能选择已验证的新 Artifact。

### 工程例外 [Conditionally Required — 超出或豁免工程规则]

N/A — 未提出工程规则例外。任何超出或豁免规则的实现都需要进行使审批
失效的 ADR 更新并重新审批，才能引入或保留该 Source。

## 契约到检查追踪 [Conditionally Required — 源代码或配置实现]

| Clause ID | 权威契约路径与标题 | 精确规范要求 | 验收检查或确定性测试 ID | 明确覆盖方法 |
| --- | --- | --- | --- | --- |
| TC-01 | 本 ADR — 规范契约条款 | 所有原生 Tool/MCP 调用路径进入 C-5；不存在直接效果路径。 | AC-1, AC-11 | 依赖/Source 检查加上 Tool 和 MCP 集成调用，证明只有一次 Executor Port 派发且零个禁用 API。 |
| TC-02 | 本 ADR — 规范契约条款 | 未知或无效元数据被拒绝，且零审批、零派发。 | AC-2 | 表测试提供每种缺失/过期/禁用/冲突用例，并断言计数保持为零。 |
| TC-03 | 本 ADR — 规范契约条款 | Turn Profile 不可变，不能被不可信内容或审批扩大。 | AC-3, AC-10 | 来自模型、描述符、决策和结果 Fixture 的篡改尝试，均不改变固定的 Profile ID/版本和决策。 |
| TC-04 | 本 ADR — 规范契约条款 | Accepted 的 D-6 恰好授权一个完全匹配的 D-7。 | AC-4, AC-5 | 精确匹配执行一次；逐字段漂移用例不产生派发，且不能复用该审批。 |
| TC-05 | 本 ADR — 规范契约条款 | 只有经过验证、具有 Scope 的同租户审批人能决议 D-6，且不泄露存在性。 | AC-6 | HTTP 契约用例覆盖缺失身份、错误租户/Thread、缺失 Scope、有效决策、重复和冲突。 |
| TC-06 | 本 ADR — 规范契约条款 | D-3 是持久的仅追加投影，绝不是权威。 | AC-3, AC-7 | 伪造/过期投影不触发派发；真实迁移在 SSE 发布前按序 Append 递增版本。 |
| TC-07 | 本 ADR — 规范契约条款 | 准备、派发和提交时检查当前租约；被 Fence 的结果不到达模型。 | AC-5, AC-8 | 在每个边界注入 Fence，断言精确的 D-7 终态/效果状态和零次结果交付。 |
| TC-08 | 本 ADR — 规范契约条款 | 只有一次经证明的效果前重试，使用新 D-7、全新策略/审批，并多消耗一个 Turn 尝试槽位。 | AC-9, AC-10 | 重试 Fixture 针对 not-started、started、unknown 和预算耗尽用例比较身份、审批次数和尝试预算消耗。 |
| TC-09 | 本 ADR — 规范契约条款 | 并发、尝试、时间和输出边界精确。 | AC-10 | 虚拟时间和字节/计数边界测试演练等于和超过每个上限的值。 |
| TC-10 | 本 ADR — 规范契约条款 | 取消关闭待决工作，或产生如实有界的 Executor 结果。 | AC-8 | 待决、运行中已确认和运行中未确认用例断言终态与派发/结果计数。 |
| TC-11 | 本 ADR — 规范契约条款 | Tool/MCP 输出不能授予权限，只有在带 Fencing 的持久提交后才交付。 | AC-3, AC-8 | 恶意输出 Fixture 加上 Append/Fence Trace 验证权限不可变和先 Append 后交付模型。 |
| TC-12 | 本 ADR — 规范契约条款 | 条件的持久迁移只允许一个获胜者，且同一个 D-7 不重复派发。 | AC-4, AC-12 | 精确审批复用和 32 路 PostgreSQL 竞争覆盖决策、逐尝试派发声明和终态提交，同时只允许 TC-08 重试使用新的 D-7 身份。 |
| TC-13 | 本 ADR — 规范契约条款 | 禁用恢复暴露不可用，且无遗留或直接 Fallback。 | AC-11 | 运行时/依赖检查和禁用 Executor 请求断言零个 Fallback 标识符和派发。 |
| TC-14 | 本 ADR — 规范契约条款 | 每条关联审计记录最多 16,384 字节，且不含凭据值和原始动作参数/结果内容。 | AC-13 | 审计 Fixture 检查断言必需的 ID、哈希、字节数、终态代码、大小不超过 16,384 字节，以及不含机密/原始内容。 |

## 风险覆盖矩阵 [Conditionally Required — 源代码或配置实现]

| 风险维度 | 适用性与场景或具体 N/A 原因 | 负责边界 | 确定性验证方法 | 精确预期结果 | 验收检查 ID | 状态 | 实际证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | 两个审批人和 32 个派发方争抢一个 requested D-6/prepared D-7，同时一个终态结果也在竞争。 | C-5 加 C-6 PostgreSQL Adapter | AC-12 生产 PostgreSQL 竞争测试。 | 一个 D-6 决策和一个派发声明获胜；一个 D-7 终态持久化；Executor 派发计数恰好为 1；每个失败者返回现有终态或 Typed 冲突。 | AC-12 | Not Started | 部分观察 — D-6 决策分支已在当前未提交任务修订上实现并竞争验证：针对一次性生产 PostgreSQL，32 个并发的 `SqlxApprovalRecordStore` 决策在一个 requested 审批上恰好产生 1 个条件 UPDATE 获胜者（版本 2）和 31 个相同的 `ExistingTerminal` 读取，且相同重放与冲突决策不改变任何状态。D-7 派发声明与终态提交分支及完整的 AC-12 命名 Harness 尚未实现，该行保持开放。 |
| timeout and deadline | 审批跨越五分钟时限或更早的 Turn Deadline；Executor 跨越 30 秒。 | C-5 时钟/Deadline 策略和 Executor Adapter | AC-10 中两条审批到期分支和执行超时的虚拟时钟边界用例。 | 在较早的适用 Deadline，pending 的 D-6 过期且零派发；running 的 D-7 为 `timed_out`；没有迟到的审批或结果到达模型。 | AC-10 | Pass | AC-10 黑盒边界表在当前未提交任务修订上经公共 `ToolExecutionBoundary` 通过：恰在 5:00.000（Turn Deadline 较晚）与 2:00.000（两分钟 Turn Deadline）到达的决议使 D-6 过期并以零派发关闭 Prepared D-7，而 4:59.999 与 1:59.999 接受并派发一次；窗口内无作用域决议被拒绝且零派发；恰在 30.000 秒完成的执行提交 `timed_out` 而不交付结果，29.999 秒成功。 |
| cancellation and interruption | Turn 在审批待决时、效果未开始时、效果已开始后，以及取消确认丢失时被中断。 | C-2/C-5 取消和 Executor Adapter | AC-8 中的故障驱动用例。 | 待决工作零次派发；已确认的运行中工作为带报告效果状态的 `cancelled`；未确认的工作为 `timed_out/unknown`；一个 CAND-1 Turn 终态保持持久。 | AC-8 | Not Started | 未在 AC-8 端到端边界运行 — 已认证取消传输是 T-2 交付物。内部取消与超时契约已实现，并由聚焦的逻辑层 Fixture 覆盖（`cargo test -p koduck-ai --lib cand_2_cancellation_tests`：prepared 中断零派发且零取消调用、已确认 not_started/started 终态、未确认 `timed_out/unknown`、两侧精确的 30 秒 Deadline 边界、取消后迟到结果拒绝、被 Fence 所有者的对账，以及使从未派发或已执行的尝试远离取消流程的 Claim 后或派发后租约不可用保留）；这些不提升 AC-8，因为它们不经过已认证传输。 |
| resource bounds and backpressure | 一个 Turn 请求第 17 次尝试、两个并发尝试、65,537 字节输入或 1,048,577 字节输出。 | C-5 策略和 Executor Envelope/结果 Adapter | AC-10 中的精确边界表。 | 每个超限用例被以其稳定上限代码拒绝或失败，零个超限字节到达历史/模型，且最多一个 D-7 运行。 | AC-10 | Pass | AC-10 黑盒边界表在当前未提交任务修订上通过：第 17 次分配与耗尽预算的重试分配返回精确的 `attempt_limit` 结果/代码且不再派发；65,537 字节动作输入在解析前返回 `InputTooLarge` 而 65,536 字节输入可执行；1,048,577 字节 Executor 输出被丢弃为 `failed/output_limit_exceeded` 且无 Payload，而 1,048,576 字节提交；同一 Turn 的第二个并发动作以恰好一次运行中派发被拒绝 `concurrent_attempt`。 |
| framework or trust-boundary rejection | 尝试缺失/伪造身份、跨租户决策、恶意 MCP 描述符/结果和直接 Executor 绕过。 | C-1/C-7、C-5、Tool/MCP/Executor Adapter | AC-1/AC-6/AC-11 中的 Axum HTTP 契约、恶意 Adapter Fixture 和架构检查。 | 身份用例返回精确的 401/404 行为且零变更；不可信内容不能改变权限；不存在禁用的直接执行路径。 | AC-1, AC-6, AC-11 | Not Started | 未运行 — 领域、Adapter 和无绕过检查已存在，但已认证 HTTP 契约和完整的恶意描述符/结果 Fixture 尚未实现 |

## 验收检查 [Required]

| 检查 ID | 子任务 | 二元验收点 | 前置条件或输入 | 验证方法 | 精确预期结果 | 预期证据 | 状态 | 实际结果与证据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | 领域/应用 Tool 策略不依赖 HTTP、Provider-wire、SQLx、Tool/MCP-wire 或 Executor 实现，且每个调用都进入 C-5。 | T-1/T-2 Source 已存在。 | 运行 `cargo test -p koduck-ai --test architecture cand_2_policy_dependencies_are_inward_and_unbypassable -- --exact`。 | Exit 0；禁用 Import 计数为 0；原生 Tool 和 MCP 入口点计数等于委托给 C-5 Port 的计数；C-1/C-2 中直接文件系统/进程/MCP 执行入口点计数为 0。 | Command Output 和所检查的 Commit。 | Not Started | 指定测试在当前未提交任务修订上 Exit 0（依赖方向向内、恰好一个生产 `impl IsolatedExecutor for DisabledExecutor`、零直接进程 API），但它并未统计原生 Tool/MCP 入口点相对 C-5 委托的计数，且在 T-2 传输之前不存在任何原生入口点，因此声明的入口委托相等性尚未被观测；该检查保持开放，直至与 T-2 入口点一同落地增强的计数测试。 |
| AC-2 | T-1 | 每个缺失、过期、禁用、不兼容、冲突、未知效果或超出 Profile 的描述符都在审批或执行前被拒绝。 | 表 Fixture 包含每种无效状态的一个用例。 | 运行 `cargo test -p koduck-ai --test cand_2_policy invalid_descriptors_fail_closed -- --exact` 和 `cargo test -p koduck-ai --lib cand_2_denial_tests`。 | 两条命令均 Exit 0；每个用例返回其声明的 Typed 拒绝；D-6 计数和 Executor 派发计数为 0。 | Command Output 和逐用例计数。 | Pass | 在当前未提交任务修订上两条命令均 Exit 0：公开策略表（1 项通过）对全部七类声明状态——缺失、过期、禁用、不兼容、冲突、未知效果和超出 Profile——返回其精确 `DenialCode`；crate 内部边界 Harness（2 项通过）以需审批的特权效果驱动同样七类，并观测到零 D-6 创建（决策回调从未被调用）、零 Executor 派发和零终态提交。 |
| AC-3 | T-1 | 模型、描述符、审批投影和 Tool/MCP 结果内容不能扩大不可变的 Turn Permission Profile 或授权执行。 | 固定 Profile 只允许合成 `read_data`；四个恶意 Fixture 请求 `process_execute`。 | 运行 `cargo test -p koduck-ai --test cand_2_policy untrusted_content_cannot_grant_authority -- --exact`。 | Exit 0；Profile ID/版本不变；四个特权请求全部被拒绝或要求规范 D-6；伪造投影导致 0 次派发。 | Command Output 和决策 Trace。 | Not Started | 当前未提交任务修订上已有三类声明恶意 Fixture Fail Closed——请求 `process_execute` 的模型内容被拒绝 `OutsidePermissionProfile`；已配置的 read_data 描述符无法重标记特权请求（`DescriptorConflicting`）；嵌入特权指令的成功读取结果不改变 Profile，其后的特权请求仍被拒绝且仅有一次 Profile 内派发——同时调用方构造的未 Seal 绑定甚至无法创建 D-6（`PolicyAuthorizationRequired`），被策略拒绝的动作不会到达决策回调或派发。但声明的伪造 D-3 审批投影 Fixture 在 T-2 投影传输存在之前无法演练，因此该检查保持开放。 |
| AC-4 | T-2 | 一个 accepted 的精确 D-6 通过 Executor Adapter 恰好授权一个匹配的 D-7。 | 一个需要审批的合成动作，固定租户/Thread/Turn/Generation/Profile/描述符/目标/参数/尝试，以及隔离 Executor Harness。 | 运行 `cargo test -p koduck-ai --lib cand_2_approval_tests::exact_approval_authorizes_one_attempt -- --exact`。 | Exit 0；精确动作派发一次；第二次使用返回 `approval-already-consumed`；每个绑定字段的漂移派发零次且需要新的策略结果/D-6。 | Command Output、ID/摘要和派发计数。 | Not Started | Pending |
| AC-5 | T-2 | 过期 Owner 不能通过 Executor Adapter 准备、派发或提交 Tool 结果。 | 隔离 Executor Harness；三个用例分别在准备前、派发前和结果提交前一刻 Fence 租约；派发后 Fixture 报告每种效果状态。 | 运行 `cargo test -p koduck-ai --test cand_2_fencing stale_owner_never_commits_tool_result -- --exact`。 | Exit 0；派发前用例发起 0 次 Executor 调用并取消 D-7/not_started；派发后 not_started 为 cancelled，started/unknown 为 failed `owner_fenced_after_dispatch`；每个用例的模型结果计数为 0。 | Command Output 和 D-7/历史 Trace。 | Not Started | Pending |
| AC-6 | T-2 | 审批决策路由强制执行精确的已认证所有权、Scope、幂等性和冲突行为。 | Requested D-6，加上缺失身份、错误租户、错误 Thread、缺失 `ai.tool.approve`、有效 Principal、重复相同决策和冲突决策。 | 运行 `cargo test -p koduck-ai --lib cand_2_http_tests::approval_decision_v1_contract -- --exact`。 | Exit 0；缺失身份为 401；错误所有权/Scope 为不可区分的 404 且零变更；有效和重复相同决策返回相同终态版本；冲突决策为 409 `approval-already-resolved`。 | Command Output、归一化响应 Fixture 和记录版本。 | Pass | Exit 0（当前未提交任务修订，1 项通过）。Harness 移入 crate 内部（`tests/internal/cand_2_http.rs`，沿用既定边界面先例），验证命令因此为 `--lib cand_2_http_tests::approval_decision_v1_contract`，属审批保全的路径维护——测试名称、输入与断言不变。通过框架中立适配器与内存规范 double 验证全部声明的身份用例：缺失身份为带 `WWW-Authenticate: Bearer` 的 `401`；无作用域同租户主体与有作用域跨租户主体均收到不可区分的 `404` 且零存储变更；五个畸形请求体在任何决策前被 `400` 拒绝；有效决策返回精确的 `{approval_id, status, decision, version: 2}` 投影；来自所有者主体的相同重放返回完全相同的响应体；冲突决策为 `409`；未知身份为 `404`，整轮恰好一次变更。适配器 `handle` 接受可信 Thread 作为路由上下文（与网关验证身份一起），错误 Thread 用例被显式执行而非结构性假设：经不同可信 Thread 到达的有作用域所有者主体收到不可区分的 `404` 且零变更（含错误 Thread 回归）。规范请求者所有权在持久查找本身强制：D-6 在插入时记录已认证 `requester_subject`，每个条件迁移与重读均以租户加请求者主体为键，因此不拥有该审批的同租户有作用域主体观察到同样不可区分的 `404` 且零变更（含错误所有者回归）。重复 `decision` 成员在任何决策前被类型化的重复字段反序列化器拒绝。该路由的 Axum 运行时接线仍是开放的 T-2 工作，不影响契约断言。 |
| AC-7 | T-2 | 审批和执行 SSE 投影是有序持久视图，绝不是独立权威。 | 一个需要审批的合成 Tool 调用被接受并完成；安装 Append/发布观察者。 | 运行 `cargo test -p koduck-ai --test cand_2_http projections_append_before_publish -- --exact`。 | Exit 0；requested、accepted、running 和 succeeded 投影引用递增的规范版本；每次 Append 先于发布；删除/伪造/重放一个投影不改变任何 D-6/D-7 且导致零次额外派发。 | Command Output 和 Append/发布 Trace。 | Not Started | Pending |
| AC-8 | T-2 | 已认证中断和租约 Fencing 产生如实的 Executor 取消结果，且无迟到结果交付。 | 审批传输和隔离 Executor Harness；待决审批；prepared 尝试；已确认 not_started/started 的运行中尝试和缺失取消确认。 | 运行 `cargo test -p koduck-ai --test cand_2_cancellation`。 | Exit 0；待决/prepared 用例派发 0 次并取消；已确认用例为带精确状态的 cancelled；缺失确认在 30 秒到达 `timed_out/unknown`；迟到结果交付计数为 0；存在一个 Turn 终态。 | Command Output、虚拟时钟 Trace 和重放。 | Not Started | Pending |
| AC-9 | T-2 | 自动重试只在经证明的 Executor 效果未开始后发生一次，获得全新授权，并再消耗一个尝试槽位。 | Executor 在效果前失败、效果已开始后失败，以及状态未知；特权和只读描述符；一个用例以 15 次在先尝试开始，使初始动作消耗槽位 16。 | 运行 `cargo test -p koduck-ai --test cand_2_retry pre_effect_retry_requires_fresh_attempt_and_policy -- --exact`。 | Exit 0；预算可用时，not_started 恰好有两个不同的 D-7 ID、消耗两个槽位，特权效果有两个不同的 accepted D-6 ID；started/unknown 有一个 D-7 且无重试；初始槽位 16 之后，重试分配/派发计数为 0，动作为 `failed/attempt_limit`。 | Command Output 和身份/审批/尝试/派发 Trace。 | Not Started | Pending |
| AC-10 | T-1 | 审批、执行、尝试、输入、输出和并发上限精确。 | 虚拟时钟；Turn Deadline 更晚时的 4:59.999/5:00.000 审批用例和两分钟 Turn Deadline 的 1:59.999/2:00.000 用例；30 秒及超过 30 秒的执行；尝试 16/17；输入 65,536/65,537 字节；输出 1,048,576/1,048,577 字节；以及两个并发动作。 | 运行 `cargo test -p koduck-ai --lib cand_2_limits_tests::exact_policy_and_execution_limits -- --exact`。 | Exit 0；Turn Deadline 更晚时审批恰好在五分钟过期，Turn Deadline 更早时恰好在两分钟过期，两种过期用例均零派发；其他等于上限的用例遵循策略；超限用例返回精确的 timeout/attempt_limit/input_limit/output_limit/concurrent_attempt 代码；没有超限 Payload 到达模型/历史，运行中尝试计数绝不超过 1。 | Command Output 和覆盖两条审批到期分支的边界表。 | Pass | 在当前未提交任务修订上经 crate 内部 `ToolExecutionBoundary` Harness Exit 0（1 项通过）：观测到的 D-6 到期在 Turn Deadline 较晚时恰为请求+300,000 ms，较早时恰为两分钟 Turn Deadline；4:59.999 与 1:59.999 的决议派发一次，而 5:00.000 与 2:00.000 取消且零派发，无作用域主体为 `NotAuthorized` 且零派发、零 D-7 分配，因此重复的无作用域请求——包括 D-6 窗口已过期的请求——不消耗完整的 16 槽位预算；29.999 秒成功而 30.000 秒提交 `timed_out`；第 16 次尝试完成，第 17 次分配与耗尽预算的重试均返回精确 `attempt_limit` 代码；65,536 字节输入可执行而 65,537 字节解析前返回 `InputTooLarge`；1,048,576 字节输出提交而 1,048,577 字节被丢弃为 `failed/output_limit_exceeded` 且无 Payload；第二个并发动作以恰好一次派发被拒绝 `concurrent_attempt`；并且经生产 `RuntimeState` 访问路径分发的两个根句柄共享一份权威目录，因此经第一个句柄声明的运行中 D-7 会拒绝第二个句柄的并发尝试且零派发。 |
| AC-11 | T-2 | Tool 和 MCP Adapter 使用同一个隔离 Executor Envelope，且禁用运行时没有直接或前身 Fallback。 | 合成原生 Tool 和 MCP 描述符，加上生产清单为空/Executor 禁用的运行时。 | 运行 `cargo test -p koduck-ai --test cand_2_execution isolated_executor_is_only_effect_path -- --exact` 和 `cargo test -p koduck-ai --test architecture cand_2_has_no_direct_or_legacy_execution_fallback -- --exact`。 | 两者均 Exit 0；每个启用的合成调用在 Harness 产生恰好一个相同的自有 Envelope；禁用运行时返回 Typed 不可用且 0 次派发；禁用的直接/遗留标识符和 API 计数为 0。 | Command Output、Envelope Fixture 哈希、依赖/配置报告。 | Not Started | Pending |
| AC-12 | T-3 | PostgreSQL 在多实例竞争下恰好允许一个决策、一个派发声明和一个终态提交。 | 设置了 `KODUCK_AI_TEST_DATABASE_URL` 的全新 PostgreSQL 数据库；Migration 此前未应用；一个 D-6/D-7 的每次迁移有 32 个竞争者。 | 运行 `cargo test -p koduck-ai --test postgres_cand_2 postgres_cand_2_transitions_are_single_winner -- --exact`。 | Exit 0；Migration 成功；每次迁移有 1 个获胜者和 31 个已存在终态/冲突结果；该 D-7 的 Executor 派发计数为 1；重放包含一个终态 D-7 投影。 | Command Output、SQL 迁移计数和重放哈希。 | Not Started | Pending |
| AC-13 | T-3 | 审计元数据完整、关联、有界，且不含凭据或原始无界内容。 | 合成凭据引用、65,536 字节参数边界、1,048,576 字节结果边界和所有终态类别。 | 运行 `cargo test -p koduck-ai --test cand_2_audit audit_is_correlated_and_content_minimized -- --exact`。 | Exit 0；每条终态记录包含声明的 ID/版本/摘要/效果状态/时序/字节数/代码；凭据值和原始参数/结果子串出现 0 次；序列化审计记录最多 16,384 字节。 | Command Output 和脱敏审计 Fixture。 | Not Started | Pending |
| AC-14 | T-3 | Additive Migration、运行时契约副本和所有仓库 Rust 检查通过，且不启用生产描述符。 | T-1 至 T-3 完成；本地测试数据库可用于集成检查。 | 运行 `cargo fmt --all --check`；`cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`；`cargo test -p koduck-ai --all-targets --all-features`；检查运行时清单。 | 所有命令 Exit 0；Migration 应用两次无错误；配置的生产描述符计数为 0；没有一次性构建产物被保留用于复用或晋级。 | Command Output、Migration 报告、清单检查和被测 Commit。 | Not Started | Pending |

允许的最终检查状态为 `Pass`、`Fail` 或 `N/A — <具体原因>`。`Fail` 会阻止
完成。只有可证明检查触发条件或前置条件不适用时，`N/A` 才有效。

## 完成检查表 [Required]

| ID | 项目 | 完成条件 | 预期证据 | 状态 | 实际证据 |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR 已审批 | 记录合格非作者审批人、审批时间和精确 `Approval Evidence: Approve`；任何可选 Approval Context Revision 仅为信息性、非约束，且准确表示获批文档 | ADR 元数据 | Complete | `@linhai` 在当前 ADR-0003 审批上下文中自声明，随后提供精确 `Approve`；元数据记录 `2026-08-12T17:56:05+08:00`。未记录 Approval Context Revision，因为获批的未提交修订尚无不可变 Commit。 |
| A-2 | 完整任务已交付 | 每个已声明子任务都有实际实施证据，每个适用验收检查均为 `Pass` 且有实际结果和证据，它们共同满足完整任务结果 | Implementation Plan 和 Acceptance Checks 行 | Not Started | Pending |
| A-3 | ADD 双向链接已同步 | CAND-2 记录本 ADR 精确路径，本 ADR 记录精确 ADD 路径和 CAND-2，双方引用一致，且 CAND-2 只有在本 ADR 达到 `Complete` 或 `Verified` 时才达到 `Complete` | ADD 候选项行、ADR 元数据和稳定修订证据 | Complete | 中英文 ADD 候选项行均以 `Selected` 记录本精确路径；ADR 元数据和中央索引记录精确 ADD 路径和 CAND-2。未发生候选项完成迁移。 |
| A-4 | 满足要求级别 | 每个 Required 章节完整，每个条件触发已评估并完成或标记 `N/A — <原因>`，Optional 章节完整或删除 | 结构化文档评审 | Complete | 2026-08-12 的结构化评审确认每个 Required 章节存在、每个条件触发已评估、无未解决的模板占位符。 |
| A-5 | 验收检查可判定 | 每个检查指定一个子任务、前置条件或输入、确定性方法、精确预期结果和证据；无无约束主观标准 | 结构化验收检查评审 | Complete | 2026-08-12 的结构化评审确认 14 项检查；每项均指定一个 T-1/T-2/T-3 子任务、精确输入、确定性方法、可观察结果和证据。 |
| A-6 | 适用时治理工程例外 | 每个超出或豁免规则在审批前都有完整例外行、负责 Owner、生命周期和验证证据；否则条件子章节记录 `N/A — <原因>` | Engineering Exceptions 子章节和受影响文件证据 | N/A — 未提出例外 | 该子章节记录显式 N/A，并要求实施发现例外时执行使审批失效的重新审批。 |
| A-7 | 契约和基线风险已覆盖 | 每个规范契约条款映射到显式检查或确定性测试，每个 Required 风险覆盖矩阵行在审批前完整，并在 review-ready 或完成前达到 Pass 或具体 N/A | Contract-To-Check Traceability、Risk Coverage Matrix、验收检查和稳定证据 | Not Started | 审批前评审确认 TC-01 至 TC-14 全部映射到 AC-1 至 AC-14，五行基线风险行结构完整。review-ready 或完成前仍需要运行时 `Pass` 证据，因此本检查项尚未完成。 |

## 补充说明 [Optional]

- 选择证据：ADD-0001 为 `Current`；CAND-1 为 `Complete`；CAND-2 曾为
  `Ready`；ADR-0001 为 `Complete`；ADR-0002 为 `Verified`；因此仓库范围
  的 ADR 串行化 Gate 允许 ADR-0003。
- Tool 效果清单对效果进行分类，但不启用任何生产描述符。测试 Fixture
  不授予运行时权限。
- 当前 T-1/T-2 Source 修订的稳定实施触点：

  | 路径 | 稳定符号或契约锚点 | 必要时的关键代码片段 | 用途 | Source 修订 |
  | --- | --- | --- | --- | --- |
  | `koduck-ai/src/domain/execution.rs` | `TurnExecutionAuthority::claim_dispatch` | N/A — 稳定方法已标识受防护迁移 | 唯一校验租约与审批的 D-7 派发声明 | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/domain/execution/authority.rs` | `TurnExecutionAuthority::interruption_snapshot`、`TurnExecutionAuthority::reserve_terminal`、`TurnExecutionAuthority::mirror_terminal` | N/A — 稳定方法已标识权威边界 | 原子中断视图及终态保留/对账所有权 | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/application/execution.rs` | `ExecutionCoordinator::execute` | `let deadline = ActionDeadline::from_started_at(started_at_millis, now());` | 派发前把 C-5 时间戳转换为相对 Executor 预算 | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/application/cancellation.rs` | `ExecutionInterrupter::interrupt`、`ExecutionCoordinator::cancel_running_attempt` | N/A — 稳定方法已标识有序取消边界 | 防止部分中断，并在外部取消请求前保留运行中终态 | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/application/terminal.rs` | `ExecutionCoordinator::commit_reserved_terminal` | `let canonical_terminal_known = matches!(error, AttemptCommitError::Conflict);` | 规范终态已获胜时保留权威并要求对账 | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/application/tool_execution.rs` | `ToolExecutionDriver::execute` | N/A — 稳定方法已标识完整重试序列 | 拥有 authorize、prepare、approve-or-cancel、dispatch 与唯一允许的效果前重试 | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/application/tool_boundary.rs` | `ToolExecutionRuntimeRoot::issue`、`ToolExecutionAssembly::boundary`、`ToolExecutionBoundary::execute` | N/A — 稳定符号已标识受控工厂与装配所有的公共 C-5 入口 | 签发运行时注入 Turn 权威根的受控工厂；每个派生 Boundary 共享该根的目录、Crate 自有 Sealing 服务与 Crate 自有 `ai.tool.approve` 作用域 Authorizer，并围绕 Consumer-owned Port | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/runtime/mod.rs` | `trust_context`、`gateway_validated_scopes` | N/A — 稳定符号已标识 C-7 网关验证作用域密封边缘 | 把网关验证审批作用域密封进 `TrustContext` 并在任何畸形作用域值上使身份无效的运行时信任边界 | 当前未提交任务修订；完成前替换为实施 Commit |
  | `koduck-ai/src/adapters/execution/postgres.rs` | `SqlxApprovalRecordStore::insert_requested`、`SqlxApprovalRecordStore::resolve_decision` | N/A — 稳定符号已标识规范 D-6 持久迁移 Adapter | 基于生产 `SQLx` pool 的条件单写者 D-6 决策迁移，含到期与租户作用域身份处理 | 当前未提交任务修订；完成前替换为实施 Commit |

- 当前任务修订的时间点分解评审；这些测量仅是评审证据，不是要求 ADR
  或 Source 在后续编辑后继续保持相等的断言：
  `koduck-ai/src/domain/execution.rs` 为 782 物理行，
  `koduck-ai/src/application/execution.rs` 为 772 物理行。两者都超过
  400 行评审阈值，且低于 800 行例外上限。领域文件保留一个精确尝试
  权威聚合：共享绑定身份、D-6 授权和 D-7 单次派发迁移，它们必须一起
  变化才能保持 TC-04/TC-08/TC-12。
  `koduck-ai/src/domain/execution/authority.rs` 为 232 物理行，拥有多 Turn
  查找、强共享尝试预算保留和 C-5 中断边界消费的已登记活跃 D-7 重建；重建的
  句柄不授予生命周期权限，因为每个受防护迁移仍会校验目录成员资格。在 T-3
  能把它绑定到规范终态持久化并防止被回收的 Turn 以全新预算重建之前，回收
  保持延后。应用文件保留使 TC-07 不可绕过所需的单一租约/准备/派发/条件提交
  边界；有界的 Executor 响应构建器与错误证据位于同级
  `koduck-ai/src/application/executor_envelope.rs` 模块（162 物理行），该模块
  拥有一份内聚的 Executor Port 契约；再抽取 Coordinator 的其余阶段会模糊
  一条失败路径而不产生独立生命周期。有界取消位于同级的
  `koduck-ai/src/application/cancellation.rs` 模块；该文件为 450 物理行，高于
  生产文件评审阈值且低于例外上限。它保留单一的中断/取消边界，其权威快照、
  终态保留、Executor 确认、Deadline 和对账规则必须保持有序；拆分这些阶段会
  产生一个没有独立生命周期的直通模块。它在外部取消请求前保留运行中的 D-7；
  共享的 `koduck-ai/src/application/terminal.rs` 为 114 物理行，低于所有文件
  评审阈值；该模块拥有派发与取消共同使用的条件提交，因此不存在第二条终态
  路径。
  `ExecutionInterrupter::interrupt` 为 87 物理行（包含意图和错误文档），
  高于方法评审阈值且低于例外上限。它把一次权威锁内快照与按序关闭全部活跃
  D-7 的结果保留在一起；抽取一个阶段会模糊“不部分关闭”的对账边界，而不会
  产生独立生命周期。`ExecutionCoordinator::cancel_running_attempt` 为 106 物理行
  （包含意图和错误文档），高于方法评审阈值且低于例外上限。它把租约检查、
  副作用前终态保留、有界 Executor 取消和如实终态选择保留在一个有序边界中。
  `ExecutionCoordinator::commit_reserved_terminal` 为 86 物理行（包含意图和错误文档），
  高于方法评审阈值且低于例外上限。它保留派发和取消共同使用的单一条件提交
  结果映射与保留释放决策；拆分会产生分叉的终态对账规则。Cyclomatic Complexity
  为 `N/A — 未配置复杂度工具`；所需替代评审测得这三个方法分别为 87、106 和
  86 物理行，最大可执行嵌套深度分别为五、二和三。中断方法超过嵌套评审阈值
  但低于例外上限；第五层是唯一有序迭代内的 prepared 尝试竞争恢复分支，抽取
  它会把过期 prepared 到 running 的刷新与其保护的原子快照分离。当 T-3 期间
  持久记录映射和 Executor 传输提供真实的抽取边界时，重新评估这两个模块和
  这些方法。
  `koduck-ai/src/domain/tool.rs` 为 658 物理行，
  高于生产文件评审阈值且低于例外上限。它保持为一个自有 Tool 值聚合，
  其 JSON 值与 Schema、描述符、动作和 Permission Profile 共享校验和
  策略不变量；现在拆分会把这些不变量移到跨模块位置而没有独立生命周期。
  `ExecutionCoordinator::execute` 为 116 物理行（含其意图和错误文档），
  高于方法评审阈值且低于例外上限。它保留一个有序的租约检查、派发、
  效果观测、Deadline 和条件提交序列；抽取某个阶段会模糊 TC-07 的顺序而不产生
  独立 Owner。Cyclomatic Complexity 为 `N/A — 未配置复杂度工具`；所需的
  替代评审测量了 116 行跨度和最大可执行嵌套深度二，低于嵌套阈值。
- `ToolExecutionDriver::execute` 为 116 物理行（含意图与错误文档），高于
  方法评审阈值且低于例外上限。它保留一条 authorize、prepare、
  approve-or-cancel、dispatch 与条件重试的序列，其顺序编码 TC-08（仅在已
  提交 `Failed{NotStarted}` 时重试）和 TC-07（被 Fence 的重试不送达结果）；
  抽取任一阶段会把重试决策状态拆分到不同 Owner 而无独立生命周期。
  Cyclomatic Complexity 为 `N/A — 未配置复杂度工具`；替代评审测量了 116 行
  跨度和最大可执行嵌套深度三（loop、match、arm），低于嵌套阈值。
  `koduck-ai/tests/internal/cand_2_execution.rs` 为 1,035 物理行，高于
  600 行测试评审阈值且低于 1,200 行例外上限；它保持为一个内聚的隔离
  执行契约 Harness，其 Executor、租约、提交方、策略和权威 Fixture 被
  Fencing、边界、竞争和终态结果用例共享。现在拆分这些用例会复制安全
  敏感的 Fixture，而不是创建独立测试边界。与 T-2 传输 Harness 一起
  重新评估；按实测大小不需要工程例外。
- `koduck-ai/tests/internal/cand_2_retry.rs` 为 761 物理行，高于 600 行
  测试评审阈值且低于 1,200 行例外上限。它是一个内聚的重试契约 Harness，
  其脚本化 Executor、租约、提交方、策略和审批 Fixture 被 not-started、
  started/unknown、至多一次、新 D-6、预算耗尽、对账、declined/cancelled
  取消、成功/取消不重试、重试期间 Fence 和时钟顺序用例共享。现在拆分会
  复制 Fixture 而不是创建独立测试边界；待 T-2 黑盒 `tests/cand_2_retry.rs`
  落地后重新评估。按实测大小不需要工程例外。
- `koduck-ai/tests/internal/cand_2_cancellation.rs` 为 1,175 物理行，高于 600 行
  测试评审阈值且低于 1,200 行例外上限。其 170 行同级
  `koduck-ai/tests/internal/cand_2_cancellation_blocking_dispatch.rs` 在复用父模块
  Fixture 的同时隔离了阻塞派发并发用例和已 Seal 派发声明的中断代码诊断；46 行的
  `koduck-ai/tests/internal/cand_2_cancellation_disabled_executor.rs` 同级模块隔离了
  生产禁用 Adapter 回归，174 行的
  `koduck-ai/tests/internal/cand_2_cancellation_post_claim_lease.rs` 同级模块隔离了
  Claim 后与派发后租约不可用的保留回归，均不使共享 Harness 超过 1,200 行例外上限。它们共同构成
  TC-09/TC-10 的逻辑级取消与超时 Harness：其脚本化 Executor、租约、提交方和共享
  运行时 Fixture 覆盖 prepared 中断、确认的 not-started/started 取消、未确认的
  `timed_out/unknown`、被 Fence Owner 的对账、精确 30 秒 Deadline 边界、迟到结果
  拒绝、非运行状态的 Typed 拒绝、未知 Turn 无操作、阻塞派发时的独立取消边界、
  过期 prepared 快照防护、D-6 已接受但尚未派发的处理、取消前终态保留、终态提交
  进行中的对账、原子中断快照、取消后 Fencing 保留、已 Seal Turn 的独立派发拒绝
  中断代码、使从未派发或已执行的尝试远离取消流程的 Claim 后或派发后租约不可用终态保留，
  以及无提前超时的不可用 Adapter 对账。待 T-2 黑盒
  `tests/cand_2_cancellation.rs` 传输 Harness 落地后重新评估父 Harness。
- `koduck-ai/tests/internal/cand_2_limits.rs`（边界面移入 crate 后为 crate 内部测试）为 1,083 物理行，超过 600 行测试评审阈值、低于 1,200 行例外上限；它是内聚的 AC-10 边界表 Harness，其脚本化计数 Executor、获胜/冲突提交方、共享租约校验器、装配派生的Boundary 与时序/固定时钟 Fixture 被每个上限族共用；拆分各族会复制安全敏感的 Executor Fixture，而不是形成独立的测试边界。其 139 行同级 `koduck-ai/tests/internal/cand_2_limits_budget.rs` 在复用父模块 Fixture 的同时隔离未授权与过期窗口的预算保留回归，使父 Harness 保持在例外上限以下。`koduck-ai/tests/internal/cand_2_denials.rs` 为 492 物理行，低于所有测试评审阈值，拥有边界级 AC-2/AC-3 拒绝与不可信内容计数器；`koduck-ai/tests/cand_2_policy.rs` 在边界 Fixture 移入 crate 后重新测量为 612 物理行——略高于 600 行评审阈值、远低于例外上限——它保留一个内聚的策略表 Harness，其描述符与动作构造器被每个策略用例共享；拆分这些表会复制这些已校验 Fixture，而不是形成独立的测试边界。`koduck-ai/src/application/tool_boundary.rs` 为 208 物理行，低于所有生产文件评审阈值；它把受控权威根工厂、Sealing 服务与作用域审批服务保留在一个有序边界中。
- `koduck-ai/tests/internal/cand_2_postgres.rs` 为 881 物理行，高于 600 行测试评审阈值、低于 1,200 行例外上限。它是内聚的规范 D-6 `PostgreSQL` Harness：多线程 runtime/pool Fixture、每进程一次的 Migration 防护、经策略授权的审批 Fixture 与字面量非法终态元组表被幂等、单写者、32 路竞争、到期、重放、身份冲突、密封审批人、Schema 回归和 version-2 升级用例共享；现在拆分这些用例会复制安全敏感 Fixture，而不是形成独立的测试边界。圈复杂度为 `N/A — 无已配置复杂度工具`；替代评审测得每个测试函数的最大嵌套深度为三，低于嵌套阈值，字面量语句表以模块级常量形式置于函数体之外。两个函数超过 60 行方法评审阈值：85 行的 `schema_rejects_illegal_terminal_tuples` 保持为对字面量非法元组表的一个扁平循环加边界插入，89 行的 `migration_0003_backfills_the_thread_owner_and_fails_on_orphans` 保持为一条线性升级叙事，其 version-2 播种并入模块级字面量常量且每个失败阶段恰好植入一个违规行、迁移应用共享一个辅助函数与一条连接，其工作/清理拆分在连接回池前恢复会话 `search_path`；抽取其余阶段会把单一场景拆散到没有独立生命周期的辅助函数中。在命名的 AC-12 `postgres_cand_2` 边界与 D-7 派发声明/终态提交分支落地时重新评估该 Harness：那些分支是真正的测试边界，应成为复用本 Fixture 的同级模块，沿用 `cand_2_cancellation*` 同级先例。
- `koduck-ai/src/runtime/mod.rs` 在 C-7 作用域信任边缘与 T-2 审批决策路由接线落地后测量为 707 物理行——591 行生产代码加一个 116 行的聚焦测试模块——高于 400 行生产文件评审阈值、低于 800 行例外上限。本次评审即本段为 T-2 传输接线预设的重新评估。它保持为一个内聚的运行时装配边界：已校验的环境配置、进程唯一的 C-5 权威根装配（`RuntimeState`）、四条自有 v1 Axum 路由及其同步、流式与审批决策请求管道，以及必须在请求头进入进程之处密封租户、主体与审批作用域的 C-7 网关验证信任边缘（`trust_context` / `gateway_validated_scopes`）；`trust_thread` 仅对客户端提供的 `X-Koduck-Thread-Id` 路由上下文做语法校验，该上下文在持久的租户、Thread 与请求者所有权检查授权之前不携带任何权威。把信任边缘抽入同级模块会把身份密封与拥有请求头解析的唯一入口分离，把配置与伺服拆开则会分割一个启动生命周期而不会形成独立的失败、所有权或生命周期边界。`run` 现为 67 行并超过 60 行方法阈值：它保持为一条顺序启动装配——带 Deadline 的迁移应用、规范 D-6 路由装配、历史与对账接线、Provider 传输、监听器与伺服——可执行嵌套为零，抽取阶段只会分割一个启动生命周期而不形成独立的失败边界。其余单元均在方法阈值以下（`handle_request` 46、`handle_stream_request` 45、`handle_approval_request` 49、`build_router` 27、`trust_context` 17、`trust_thread` 14、`gateway_validated_scopes` 27 行）；圈复杂度为 `N/A — 无已配置复杂度工具`，信任边缘单元的最大嵌套深度保持为三，低于嵌套阈值。在其余 T-2 投影与 Provider Tool 调用传输消费者落地时再次重新评估该模块。
- 仓库在之前的 Pull Request 修订上已有配置的自动评审证据。ADR-0003
  尚无已推送的实施修订；因此精确修订的评审覆盖仍待完成，并仍是后续
  review-ready 的 Gate。

## 归档 [Conditionally Required — Decision Status 为 `Rejected`，或 Decision Status 为 `Deprecated`/`Superseded` 且 Implementation Status 为终态]

本节当前未激活，因为 Decision Status 为 `Accepted` 且 Implementation
Status 为 `In Progress`。触发时：

- [ ] 将英文权威文件移动到
      `docs/adr/archive/ADR-0003-default-deny-tool-approval-execution-boundary.md`；
      本翻译保留在当前路径，并把英文权威链接更新为
      `../../archive/ADR-0003-default-deny-tool-approval-execution-boundary.md`，
      同时更新归档后英文文件指向本翻译的相对链接。
- [ ] 把引用归档前英文路径的所有受治理 Source Marker 与交叉引用更新为
      归档路径。
- [ ] 若被取代，同步双向 `Supersedes` / `Superseded By` 路径。
- [ ] 无替代记录时保留 `Superseded By: None`。
- [ ] 更新 `docs/adr/INDEX.md` 中本记录唯一行的最终状态与路径；不得为
      本中文翻译新增独立行。
- [ ] 确认没有存续记录或 Code Marker 继续引用归档前路径。

## 变更日志 [Required]

| 日期 | 变更 | 作者 |
| --- | --- | --- |
| 2026-08-15 | 修正运行时分解措辞：网关验证信任边缘只密封租户、主体与审批作用域（`trust_context` / `gateway_validated_scopes`）；`trust_thread` 仅对客户端提供的 `X-Koduck-Thread-Id` 路由上下文做语法校验，该上下文在持久的租户、Thread 与请求者所有权检查授权之前不携带任何权威——与契约副本一致。zh-CN 已同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-15 | 在 T-2 审批路由接线后重跑了预设的运行时装配分解评审：`koduck-ai/src/runtime/mod.rs` 重测为 707 物理行（591 行生产代码加 116 行聚焦测试模块），含四条自有 v1 路由；`run` 现为 67 行并超过 60 行方法阈值——记录为一条可执行嵌套为零的顺序启动装配，不抽取阶段，因为这只会分割一个启动生命周期而不形成独立的失败边界。信任边缘单元嵌套深度保持为三，其余单元均在方法阈值以下（`handle_request` 46、`handle_stream_request` 45、`handle_approval_request` 49、`build_router` 27、`trust_context` 17、`trust_thread` 14、`gateway_validated_scopes` 27 行）。下一次重评触发点为其余 T-2 投影与 Provider Tool 调用传输接线。zh-CN 已同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-15 | 处理孤儿断言隔离评审发现：升级回归现在每个失败阶段恰好植入一个违规行——孤儿待决行单独驱动第一次迁移应用，空白所有者行在孤儿解决后才引入，因此每个 `is_err` 断言独立证明其目标条件（对仅拒空白的谓词观察到红灯：隔离后的空白阶段断言失败，而孤儿阶段行为正确）。Harness 分解证据已重测（881 行；升级测试为 89 行）。zh-CN 已同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-15 | 验证隔离修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、针对一次性容器的 8 项 PostgreSQL Harness 测试（容器随后删除）、无 `KODUCK_AI_TEST_DATABASE_URL` 的完整套件，以及治理校验均通过。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-15 | 处理连接池评审发现：version-2 升级回归现在在连接返回 `PgPool` 前把会话 `search_path` 恢复为 `public`——工作/清理拆分使清理在任一升级阶段失败时也会运行，并以一条非限定探针证明恢复后的连接能解析 `public.tool_approvals`（未重置时观察到红灯：探针失败 `42P01 relation "tool_approvals" does not exist`，正是后续复用该连接的并行测试会遇到的失败）。Harness 分解证据已重测（881 行；升级测试现为 89 行）。zh-CN 已同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-15 | 验证连接恢复修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、针对一次性容器的 8 项 PostgreSQL Harness 测试（容器随后删除）、无 `KODUCK_AI_TEST_DATABASE_URL` 的完整套件，以及治理校验均通过。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-15 | 处理请求者所有权与证据的三条评审发现：Migration 0003 现在对 `requester_subject` CHECK 和孤儿守卫都应用与审批人列相同的 POSIX `[^[:space:]]` 非空白谓词，使空白-only 的遗留 Thread 所有者或原始存储插入都无法创建任何有效主体无法决议的审批（Schema 元组表新增空格/tab/换行字面量回归，version-2 升级回归新增空白所有者待决行；对仅拒空白的谓词观察到红灯——空白请求者行被存入且空白所有者升级成功——修复后转绿，针对一次性生产 PostgreSQL，容器已删除）。AC-6 的 Pass 证据现描述当前修订：五个畸形请求体、来自所有者主体的相同重放、经适配器可信 Thread 路由上下文显式执行的错误 Thread 用例，以及持久请求者所有权与重复成员覆盖。881 行 `cand_2_postgres.rs` 的分解评审记录了两个超过 60 行方法阈值的函数（85 行 Schema 元组循环、89 行线性升级叙事——其 version-2 播种与迁移应用已抽取为模块级常量与一个共享辅助函数）及其内聚理由。zh-CN 已同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-15 | 验证请求者所有权与证据修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、针对一次性容器的 8 项 PostgreSQL Harness 测试（容器随后删除）、无 `KODUCK_AI_TEST_DATABASE_URL` 的完整套件，以及治理校验均通过。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-15 | 用升级回归处理请求者所有权回填评审发现：Migration 0003 不再为遗留行分配占位 `legacy-requester` 主体——它按租户加 Thread 身份从 `threads.subject_id` 回填真实所有者，任何没有匹配 Thread 所有者的行会使迁移响亮失败并留给 Operator 处理，而不会变成可被伪造主体决议（TC-05/TC-12）。新的 `migration_0003_backfills_the_thread_owner_and_fails_on_orphans` Harness 测试在专用 Schema 中以一条待决行重放 version-2 升级路径：对占位迁移观察到红灯（孤儿行被静默分配占位主体且迁移成功），修复后转绿；Harness 迁移列表现包含 `0001_cand_1_history.sql`，因为回填需要连接 `threads`。针对一次性生产 PostgreSQL 验证（容器已删除）：全部 8 项 Harness 测试通过。zh-CN 已同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-15 | 验证回填修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、针对一次性容器的 8 项 PostgreSQL Harness 测试（容器随后删除）、无 `KODUCK_AI_TEST_DATABASE_URL` 的完整套件，以及治理校验均通过。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-15 | 处理决策路由增量上的两条 P1 评审发现：Migration 0002 恢复为 HEAD 中已提交的内容（迁移历史不可变），已认证 `requester_subject` 列改经幂等前向迁移 `0003_cand_2_requester_ownership.sql` 落地（安全回填加先删后建的非空约束），Harness 通过对一次性生产 PostgreSQL 依次应用 0002 与 0003 各两次验证升级路径；且 Thread 所有权现于持久查找中强制——`resolve_decision` 携带可信 Thread 身份，每个条件迁移、既有终态读取、到期迁移与冲突重读均以租户加 Thread 加请求者主体为键，因此经另一 Thread 到达的有作用域所有者主体观察到不可区分的 `404` 且零变更（含错误 Thread 回归；不再需要事后 C-5 检查撤销已提交的 D-6）。展示服务器与网关验证身份一起提供可信 Thread 作为路由上下文。zh-CN 已同步。验证：fmt、严格 Clippy、有无 `KODUCK_AI_TEST_DATABASE_URL` 的完整套件（针对一次性 PostgreSQL 的 97 项 crate 内部 CAND-2 测试，容器已删除）、治理校验。未改变已批准决策内容。 | @zcode |
| 2026-08-15 | 处理决策路由的三条评审发现：规范请求者所有权现已持久化——Migration 增加已认证 `requester_subject` 列，插入时记录，且每个条件迁移、既有终态读取、到期迁移与冲突重读均以租户加请求者主体为键，因此同租户有作用域的非所有者观察到不可区分的 `404` 且零变更（含错误所有者回归；接线期间的失败模式均针对一次性生产 PostgreSQL 修复）；重复 `decision` 成员经类型化 `deny_unknown_fields` 反序列化器（serde 重复字段检测）拒绝，而非折叠的末值解析（含回归）；`400` problem 响应携带正确的 `Invalid request` 标题。zh-CN 已同步。验证：fmt、严格 Clippy、有无 `KODUCK_AI_TEST_DATABASE_URL` 的完整套件（针对一次性 PostgreSQL 的 97 项 crate 内部 CAND-2 测试，容器已删除）、治理校验。未改变已批准决策内容。 | @zcode |
| 2026-08-15 | 执行当前任务选定的下一个依赖顺序 T-2 增量：基于规范 D-6 存储的已认证审批决策路由，由暂存的 T-3 D-6 持久化与密封 `ApproverId` 能力解锁。交付 `ApprovalDecisionRoute`（密封能力授权，无作用域主体获得不可区分 `NotFound` 且零存储调用；条件持久结果映射为 `Resolved`/`Conflict`/`NotFound`/`Unavailable`）、`POST /api/v1/ai/approvals/{approval_id}/decisions` 的框架中立 `ApprovalDecisionAdapter`（精确仅 `decision` 请求体；`401`/`404`/`409 approval-already-resolved`/`503` 映射），以及基于内存规范 double 的 crate 内部 `tests/internal/cand_2_http.rs` 契约 Harness。AC-6 提升为 `Pass`，命令按审批保全路径维护记录（crate 内部 Harness，既定先例），错误 Thread 维度记录为结构性覆盖——路由不接受 Thread 输入；Thread 所有权规范属于 D-6 并由 C-5 Driver 在派发时校验。Axum 运行时接线仍是开放的 T-2 工作。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、完整 all-target/all-feature 套件（17 个测试二进制）以及治理校验通过。未改变已批准决策内容。 | @zcode |
| 2026-08-15 | 处理未提交 D-6 持久化增量上的最后一条评审发现：不可变字段重放比较不再使用 `unsigned_abs()`——`lease_generation`、`requested_at_millis` 与 `expires_at_millis` 在比较链之前经新的 `canonical_non_negative` 辅助函数（`try_get` 加 `u64::try_from`）以 Fail-closed 方式解码，因此 Schema 漂移产生的负持久值（如 `-5`）表现为类型化的 `Unavailable`，而不会与期望的 `5` 判等并被报告为相同重放（评审识别的防御性修正；产生该漂移行需要约束所禁止的 Schema 漂移，故无可复现红灯状态，由既有的重放断言承载覆盖）。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、有无 `KODUCK_AI_TEST_DATABASE_URL` 两种条件下的完整套件（针对一次性生产 PostgreSQL，容器已删除）以及治理校验通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-15 | 处理未提交 D-6 持久化增量上的最后两条评审发现：`row_version` 现以 Fail-closed 方式解码规范版本——改用 `try_get`、拒绝小于 1 的值并返回在每个调用点传播的 `Result<u64, ApprovalStoreError>`，使 Schema 漂移或部分 rollout 表现为 `Unavailable`，而非 panic 或伪造的正版本号（评审识别的防御性修正；该失败模式需要 Schema 漂移、无可复现红灯状态，因此由既有的投影断言覆盖而非新增回归）；且 zh-CN 的 2026-08-14 变更日志条目恢复忠实翻译当天的中间 `btrim` 状态，POSIX 正则过渡仅记录在 2026-08-15 行，使两份证据历史一致。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、有无 `KODUCK_AI_TEST_DATABASE_URL` 两种条件下的完整套件（针对一次性生产 PostgreSQL，容器已删除）以及治理校验通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-15 | 处理未提交 D-6 持久化增量上的两条后续评审发现：终态审批人 CHECK 改用 POSIX `[^[:space:]]` 正则取代仅处理空格的 `btrim`，使 tab 与换行纯空白审批人身份与 Rust `trim().is_empty()` 不变量同样被拒绝（修复前针对旧 CHECK 观察到 RED，并在 Schema 元组表中加入 tab-only 与 newline-only 字面量回归）；同时为 685 行的 `tests/internal/cand_2_postgres.rs` Harness 在 Supporting Notes 记录所要求的分解评审，含内聚性理由、替代复杂度信号与 AC-12/D-7 重评触发点；zh-CN 证据已同步。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、有无 `KODUCK_AI_TEST_DATABASE_URL` 两种条件下的完整套件（针对一次性生产 PostgreSQL 的 7 项 Harness 测试，容器已删除）以及治理校验通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-14 | 处理未提交 D-6 持久化增量上的三条后续 T-3 评审发现：审批权威不可伪造——`ApproverId` 成为 crate 密封能力，其唯一构造器 `from_authenticated(&TrustContext)` 要求携带网关验证 `ai.tool.approve` 作用域的非空已认证主体（`TOOL_APPROVAL_SCOPE` 移至领域并由应用策略模块重导出），新增 `approver_identity_cannot_be_minted_outside_the_crate` 架构回归固定无公有构造器；Migration CHECK 现强制完整终态元组（`btrim(approver) <> ''`、已决议终态的非空 `decided_at_millis` 严格早于到期、requested/expired 行的 decision/approver/decided-at 均为 NULL），五条字面量 Schema 回归覆盖每个非法元组；`ApprovalInsertResolution::Existing` 现返回规范 `{status, decision, version}` 投影，使另一实例终态或到期迁移之后的重放可无歧义对账，由新的终态后重放回归证明。zh-CN 翻译已同步。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、有无 `KODUCK_AI_TEST_DATABASE_URL` 两种条件下的完整套件（针对一次性生产 PostgreSQL 的 7 项 Harness 测试，容器已删除）以及治理校验通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-14 | 处理未提交 D-6 持久化增量上的全部三条 T-3 评审发现：`insert_requested` 现具备重放安全——`ON CONFLICT DO NOTHING` 加不可变字段校验对相同的丢失确认重放报告 `ApprovalInsertResolution::Existing`，对新类型 `ApprovalStoreError::IdentityConflict` 报告身份不匹配（评审识别的失败模式；回归已添加并针对一次性生产 PostgreSQL 验证）；`resolve_decision` 改为接收新的拒绝空白的领域类型 `ApproverId` 而非原始 `&str`，Migration CHECK 现拒绝任何审批人空白或缺失的终态行，以 Schema 作为纵深防御在持久路径保留 `apply_validated_decision` 不变量；且 T-3 证据不再把部分 D-6 Schema 描述为完整——已认证请求者主体身份与有界展示摘要均记录为随 T-2 传输/投影增量落地的开放字段。zh-CN 翻译已同步。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、有无 `KODUCK_AI_TEST_DATABASE_URL` 两种条件下的完整套件（针对一次性 PostgreSQL 的 5 项 Harness 测试，容器已删除）以及治理校验通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-14 | 以当前任务选定的首个依赖顺序增量启动 T-3：规范 D-6 `PostgreSQL` 持久化分支。交付幂等 Additive Migration `0002_cand_2_policy_execution.sql`（完整 ADR D-6 字段集、状态/决策 CHECK、待决查找索引；请求者主体列留待拥有调用方身份的 T-2 传输）、带类型化 `Won`/`ExistingTerminal`/`NotFound` 条件结果的 Consumer-owned `ApprovalRecordStore` Port、按既有 pool/runtime/Deadline 模式实现的生产 Adapter `SqlxApprovalRecordStore`、`ApprovalRequest.requested_at_millis` 与绑定 `turn_id`/`lease_generation` 持久化访问器，以及环境门控的 `tests/internal/cand_2_postgres.rs` Harness。针对一次性生产 PostgreSQL 验证（验证后已删除容器）：Migration 幂等性、相同重放与冲突决策收敛、32 路竞争恰好 1 个获胜者加 31 个相同终态读取、以及精确到期边界（Deadline 处 `expired`，窗口内决策仍获胜）。开发中观察到 RED：初始状态 CHECK 拒绝无决策的 `expired` 终态。`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy 以及完整 `koduck-ai` 套件在有无 `KODUCK_AI_TEST_DATABASE_URL` 两种条件下均通过。AC-12 保持 `Not Started`，待其命名 Harness 与 D-7 分支；未改变已批准决策内容。 | @zcode |
| 2026-08-14 | 处理未提交增量上的最后一条评审发现：为 C-7 作用域信任边缘落地后增至 606 物理行（490 行生产代码加 116 行聚焦测试模块）的 `koduck-ai/src/runtime/mod.rs` 记录了所要求的分解评审。Supporting Notes 现记录该模块为何保持为一个内聚的运行时装配边界——配置、唯一的 C-5 权威根装配、自有 v1 路由/伺服管道与入口所有的信任边缘——抽取为何会恶化耦合、方法跨度与嵌套替代信号，以及 T-2 重评触发点；zh-CN 证据已同步。仅移动测试块仍会使生产跨度高于 400 行阈值，因此记录评审是合规处置。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、完整 `koduck-ai` all-target/all-feature 套件以及治理校验通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-14 | 处理未提交作用域密封边缘上的后续评审发现：`gateway_validated_scopes` 中的 16 Token 上限现在在复制 Token 之前强制执行，而非在整个向量构建完成之后，因此巨大的逗号分隔畸形头在拒绝前至多分配 16 个 Token，使计数上限成为真正的资源上限。可观察契约不变——既有的 17 Token 拒绝用例仍是回归覆盖——且公共行为无差异，故不存在新的红灯状态；该变更为控制流修正加上记录在案的资源上限。zh-CN 翻译已同步。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy 以及完整 `koduck-ai` all-target/all-feature 套件通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-14 | 以 Test-first 修正处理未提交 C-7 作用域信任增量上的两条评审发现：`koduck-ai/docs/runtime-configuration.md` 中可强制的网关剥离/重发交接现已显式覆盖 `X-Koduck-Approval-Scopes`（网关必须移除调用方提供的值，且只从已验证签名 Claims 作用域设置该头；无法强制该规则的部署不得暴露审批决策路由）；`gateway_validated_scopes` 不再归一化首尾空白——Token 按原样校验，`" ai.tool.approve"` 使身份无效而非被 trim 成特权作用域（观察到 RED：带前导空格的 Token 被接受），与契约已声明的 `[A-Za-z0-9._:-]` 语法一致。契约副本与 zh-CN 翻译已同步。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、完整 `koduck-ai` all-target/all-feature 套件以及治理校验通过。未改变已批准决策内容或验收状态。 | @zcode |
| 2026-08-14 | 执行当前任务选定的下一个依赖顺序 T-2 增量：按仓库 Owner 指示（明确未选择服务内 JWT/JWKS 验证）以延续 CAND-1 网关验证上下文通道的方式实现 C-7 审批作用域信任边缘。运行时 `trust_context` 边界现把 `x-koduck-approval-scopes` 中的作用域经 crate 内部 `ApprovalScopes::from_validated` 密封（至多 16 个 Token、每个至多 128 ASCII 字节；头缺失即无作用域；存在但不可读或畸形的值以 `401` 使整个身份无效），`from_validated` 的 dead-code 豁免已移除，`koduck-ai/docs/contracts/cand-2-tool-approval-v1.md` 记录了该通道。修复前观察到 RED：作用域头被忽略且畸形值被静默降级为无作用域。验证：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy 以及完整 `koduck-ai` all-target/all-feature 套件通过。未改变已批准决策内容或验收状态；T-2 保持 `In Progress`，待决策路由、SSE/D-3 投影和 Provider Tool 调用翻译完成。 | @zcode |
| 2026-08-14 | 用 Test-first 回归处理两个阻塞评审发现：过期 D-6 窗口不再豁免 C-7 校验——新的绑定级 `validate_resolver_for_binding` 在分配后取消路径之前运行，过期窗口的无作用域循环零消耗尝试槽位（观察到红灯：首个过期无作用域调用在消耗槽位后返回 `cancelled/not_started`）；派发后租约 `Unavailable` 现在为对账保留已执行尝试的终态保留，租约恢复后中断无法取消 Executor 已运行的效果（观察到红灯：中断提交了 `cancelled/started`）。C-7 Authorizer Port 改为接收精确绑定，使校验在 D-6 请求存在之前即可运行；两个预算回归位于 139 行的 `cand_2_limits_budget.rs` 同级模块，使上限 Harness 保持在 1,200 行例外上限以下。分解证据已重新测量，zh-CN 翻译已重新同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-14 | 验证阻塞发现修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy，以及完整 216 个测试的 `koduck-ai` all-target/all-feature 套件通过，其中包括 89 个聚焦 CAND-2 库测试和 14 个架构契约测试。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-14 | 用 Test-first 回归处理两个阻塞评审发现：C-7 所有权与作用域预校验现在在任何 D-7 分配之前运行，未授权的需审批调用不再留下已准备尝试，重复的无作用域请求无法耗尽 16 槽位预算（观察到红灯：十六次无作用域调用后首个已授权尝试命中 `attempt_limit`）；Claim 后租约 `Unavailable` 现在为对账保留运行中尝试的终态保留，不再把从未派发的尝试暴露给取消流程（观察到红灯：中断对其提交了 `cancelled/not_started`）。新的取消回归位于 84 行的 `cand_2_cancellation_post_claim_lease.rs` 同级模块，使父 Harness 保持在 1,200 行例外上限以下；分解证据已重新测量，zh-CN 翻译已重新同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-14 | 验证阻塞发现修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy，以及完整 215 个测试的 `koduck-ai` all-target/all-feature 套件通过，其中包括 88 个聚焦 CAND-2 库测试和 14 个架构契约测试。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-14 | 处理运行时根与证据评审轮次：生产运行时现在在启动时装配一个 `RuntimeState`，持有进程唯一的 C-5 Turn 权威根且只分发共享句柄，取代按调用签发；新增 AC-10 回归经生产访问路径驱动两个句柄，证明共享目录拒绝跨句柄并发尝试且零派发（修复前观察到意外的 `Succeeded` 红灯）。AC-2 的验证方法现在列出两条已执行命令（公开策略表加 crate 内部拒绝 Harness），相互矛盾的过期分解测量（已移除的外部 `tests/cand_2_limits.rs` 引用、970 行策略与 192 行边界的重复旧值、较早的 Coordinator/Driver 方法跨度）已删除或按当前 Source 重新测量；zh-CN 翻译已重新同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-14 | 验证运行时根与证据修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、14 个架构契约测试、两条 AC-2 命令（分别 1 项和 2 项通过）、87 个聚焦 CAND-2 库测试，以及完整 214 个测试的 `koduck-ai` all-target/all-feature 套件通过。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-14 | 通过封闭两条权威路径处理第四轮评审：根签发转为 crate 内部（`ToolExecutionRuntimeRoot::issue` 为 `pub(crate)`，运行时装配的 `tool_execution_root` 是唯一生产调用点，由架构断言固定），边界面（`ToolExecutionAssembly`/`ToolExecutionBoundary`）移入 crate 内部，任何调用方都无法铸造第二份权威目录；审批作用域成为封闭的 `ApprovalScopes` 能力，其构造器 crate 内部私有，`TrustContext::with_approval_scopes` 是唯一附加途径，因此任何外部调用方都无法铸造 `ai.tool.approve`（回归由编译器私有性加架构断言强制）。AC-10 边界 Harness 移至 `tests/internal/cand_2_limits.rs`，边界级 AC-2/AC-3 计数器移至 `tests/internal/cand_2_denials.rs`；AC-10 验证命令更新为 `cargo test -p koduck-ai --lib cand_2_limits_tests::exact_policy_and_execution_limits -- --exact`，属沿用第九轮评审 AC-4 先例的审批保全路径维护——测试名称、输入与精确断言不变，仅位置移入 crate 内部。 | @zcode |
| 2026-08-14 | 处理未提交修订上的第三轮评审：以显式注入、经 `ToolExecutionRuntimeRoot::issue` 受控工厂签发的 `ToolExecutionRuntimeRoot` 取代第二轮引入的进程全局 `PROCESS_AUTHORITY_ROOT`——宿主运行时拥有签发权且不存在全局根，由更新后的架构断言强制——跨 Assembly 回归改为证明同一注入根上的共享；并以类型化 `LeaseCheck` Port 取代锁中毒时伪装成 Fenced 的 Fail Closed，其 `Unavailable` 结果以新增的 `ExecutionFailure::LeaseUnavailable` / `ExecutionPreparationError::LeaseUnavailable` 对账传播、零派发且不伪装为 Fence（锁中毒回归断言该类型化结果）。执行器信封类型移入 `application/executor_envelope.rs`，使 `execution.rs` 保持 686 物理行，低于 800 行例外上限。 | @zcode |
| 2026-08-14 | 以三项红-绿回归处理未提交修订上的第二轮评审：决策提供方现仅在 `ApprovalDecisionService::validate_resolver` 确认租户、Thread 和 `ai.tool.approve` 作用域之后被调用，未授权主体回调计数为 0 且不暴露 D-6（TC-05）；每个 `ToolExecutionAssembly` 解析由架构断言固定的进程共享 `PROCESS_AUTHORITY_ROOT`，跨 Assembly 回归证明第二个 Assembly 继承同样的运行中尝试与 16 槽位限制（TC-09/TC-12）；中毒的共享租约锁以 Fenced 方式 Fail Closed 且零派发，而不是复用 panic 校验器的状态（TC-07）。AC-3 因其声明的伪造 D-3 审批投影 Fixture 依赖 T-2 投影传输而回到 `Not Started`。 | @zcode |
| 2026-08-14 | 以红-绿增量新增公共 C-5 执行入口，并处理未提交修订上的全部五项评审发现：`ToolExecutionAssembly` 现持有唯一 Turn 权威根，每个 `ToolExecutionBoundary` 均由它派生，因此一个 Turn 跨端口特定 Boundary 恰好保留一个 16 槽位预算和一个 running D-7（新增 `concurrent_attempt` 与 `attempt_limit` 跨 Boundary 回归）；C-5 Driver 在策略评估和 D-7 分配前拒绝租户不匹配的调用——包括免审批的 `read_data` 路径——返回精确 `TenantMismatch` 代码、零 D-6 创建和零派发（新增跨租户回归）；AC-2 经边界覆盖全部七类声明拒绝并带零 D-6/派发/提交计数；AC-3 覆盖全部四类声明恶意 Fixture 且特权零派发；因 Runner 集成未交付且 T-1 范围未修订，T-1 回到 `In Progress`；因指定测试未统计入口委托且 T-2 之前无原生入口点，AC-1 回到 `Not Started`。 | @zcode |
| 2026-08-14 | 验证评审修正后的增量：`git diff --check`、`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、精确的 AC-2/AC-3/AC-10 命令及 AC-1 命令结果，以及完整 212 个测试的 `koduck-ai` all-target/all-feature 套件通过；两个既有 Provider 超时测试使用了被允许的回环绑定。 | @zcode |
| 2026-08-14 | 用 Test-first 回归处理中断诊断与测试确定性评审：已 Seal 的 Turn 的被拒派发声明现在报告独立的 `ExecutionFailure::InterruptionRequested` 代码，而非审批不匹配诊断；已 Seal 声明回归改为用 `recv_timeout` 确定性等待阻塞取消服务发出的封印后信号，取代固定 50ms 休眠，并在释放前证明“已封印但仍为 Prepared”的关键中间状态。分解证据已按当前 Source 重新测量，zh-CN 翻译的分解评审块已重新同步。未改变任何已批准决策内容或验收状态。 | @kimi |
| 2026-08-14 | 验证中断诊断修正：`cargo fmt --all --check`、严格 `koduck-ai` all-target/all-feature Clippy、31 个聚焦取消回归，以及完整 211 个测试的 `koduck-ai` all-target/all-feature 套件通过；治理验证器测试与仓库治理校验通过。未把任何未完成的验收检查提升为 `Pass`。 | @kimi |
| 2026-08-14 | 用 Test-first 回归处理取消收敛评审发现：`DisabledExecutor` 返回独立的不可用结果；目录会原子封闭已知和未知的已中断 Turn，阻止后续 D-7 分配；派发前终态提交失败时会保留已处于 running 的权威保留，使中断无法取消从未派发的 Executor 动作。聚焦取消测试已拆入按职责命名的同级模块，使共享 Harness 保持在 1,200 行例外上限以下。聚焦及完整 Rust 测试、格式化、严格 Clippy、治理测试和仓库治理校验均通过。 | @codex |
| 2026-08-13 | 用 Test-first 回归收窄有界取消确认类型：`CancelAcknowledgement::Acknowledged` 现仅携带仅含 `NotStarted` 或 `Started` 的 `CancelledEffectState`，使已确认取消只能提交定义明确的 `cancelled/not_started` 或 `cancelled/started` 终态。无法确认的效果报为 `NotAcknowledged` 并提交 `timed_out/unknown`；`cancelled/unknown` 不再可达。新增 `acknowledged_cancellation_only_commits_defined_cancelled_terminals` 回归。T-1/T-2 保持 `In Progress`；未提升任何验收检查。 | @zcode |
| 2026-08-13 | 用 Test-first 回归处理取消期限评审发现：有界取消路径现会在 Executor 取消返回后重新读取 C-5 时钟，使在 30 秒动作期限之后到达的确认提交 `timed_out/unknown` 而非 `cancelled`。新增 `late_cancellation_acknowledgement_commits_timeout_with_unknown_effect` 回归。T-1/T-2 保持 `In Progress`；未提升任何验收检查。 | @zcode |
| 2026-08-13 | 用 Test-first 回归处理派发后终态提交评审发现：当已请求 Executor 效果（`AfterDispatch`）时，条件规范终态写入若以 `Unavailable` 或 `Fenced` 失败，现在会为对账保留 D-7 权威保留，而不再把 running D-7 释放回竞争的中断器，从而防止第二次取消提交矛盾终态。新增 `post_dispatch_durability_failure_keeps_the_running_attempt_reserved_against_interruption` 回归。T-1/T-2 保持 `In Progress`；未提升任何验收检查。 | @zcode |
| 2026-08-13 | 用稳定的完全限定实施触点替换持续同步的 Source 行数与方法跨度证据，并在符号本身不足时保留一段决定性关键代码。分解测量明确降为时间点评审证据，Rust 架构测试不再把 ADR 措辞或行数与当前 Source 比较。未改变任何已批准决策、范围或验收状态。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理 prepared 取消竞争和派发前到期评审发现：prepared 取消输给并发 `prepared -> running` 迁移后，C-5 会刷新该精确的已登记 D-7 并发送一次有界的 running 取消；剩余动作预算为零时，以零 Executor 派发提交 `timed_out/not_started`。已重新测量受影响的 Source/测试证据。T-1/T-2 保持 `In Progress`；未提升任何验收检查。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理终态提交和时钟域评审发现：`Conflict` 结果现保留 D-7 权威保留，因为它表明另一规范终态已经获胜，使镜像在对账完成前保持不可用；C-5 时间戳在 Executor 派发或取消前被转换为相对的剩余 30 秒动作预算，因此 Executor 在自己的单调时钟域中应用该预算。已重新测量受影响的 Source/测试证据。T-1/T-2 保持 `In Progress`；未提升任何验收检查。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理两个取消/终态提交评审发现：规范终态已获胜但本地 D-7 镜像无法更新时，其权威保留会持续到对账完成，因而不会重新开放派发或取消；阻塞派发的中断回归现在使用独立的真实取消 Coordinator，并在释放派发前证明恰好一次 Executor 取消和一次持久终态提交。已重新测量受影响的 Source/测试证据。T-1/T-2 保持 `In Progress`；未提升任何验收检查。 | @codex |
| 2026-08-13 | 实现 T-1 取消和超时契约：隔离 Executor Port 在不透明 Permit 后新增一次有界取消；运行中 D-7 的中断校验当前租约，并按确认的效果状态提交终态，未确认时提交 `timed_out/unknown`；每个 Executor 响应均相对 30 秒动作上限检查，超时响应提交 `timed_out` 且不交付输出；中断句柄从共享进程权威解析已登记的活跃 D-7；`DisabledExecutor` 对取消 Fail Closed。聚焦的 `cand_2_cancellation_tests` 覆盖 TC-09/TC-10 逻辑；T-1 仍因生产接线和 Runner 集成未完成而保持 `In Progress`。 | @zcode |
| 2026-08-13 | 验证取消和超时增量：`cargo fmt --all --check`、严格 all-target/all-feature Clippy，以及完整 192 个 `koduck-ai` all-target/all-feature 测试通过，其中包括 9 个新的 `cand_2_cancellation_tests` Fixture 和更新后的 ADR 证据架构检查。未把任何生产边界前置条件尚未完成的验收检查提升为 `Pass`。 | @zcode |
| 2026-08-13 | 处理取消评审发现：中断在关闭匹配的 prepared D-7 前，经必需的待决审批 Port 关闭精确 D-6；一次中断枚举并关闭所有已登记的活跃 D-7；prepared 取消在提交前重验租约；派发与取消均显式接收有界的 30 秒动作预算。新增回归覆盖精确 D-6 取消、多个 prepared D-7、prepared Fencing 和 Deadline 传递。T-1/T-2 保持 `In Progress`，未提升任何验收检查。 | @codex |
| 2026-08-12 | 起草项目级 Full ADR，选择 ADD-0001 CAND-2，并定义默认拒绝、精确尝试的 Tool/MCP 审批与隔离执行契约。 | @codex |
| 2026-08-12 | 处理审批前评审的精确性问题：演练两条审批到期分支，使 AC-12 可执行，使审计上限规范化，按 D-7 限定重复派发措辞，把重试计入尝试预算，并将依赖 Adapter 的检查对齐到 T-2。 | @codex |
| 2026-08-12 | 人类审批人在当前 ADR-0003 上下文中自声明 `@linhai` 并提供精确 `Approve` 后接受；记录 Approval Time `2026-08-12T17:56:05+08:00`。Implementation Status 保持 `Not Started`；由于获批的未提交修订尚无不可变 Commit，不记录 Approval Context Revision。 | @linhai |
| 2026-08-12 | 把完成检查表 A-7 从 `Complete` 更正为 `Not Started`，因为其五行风险覆盖矩阵在 review-ready 或完成前需要运行时 `Pass` 证据；本次证据状态更正不改变任何已批准决策内容。 | @codex |
| 2026-08-12 | ADR 接受后进入 Implementation Status `In Progress`，并开始 Test-first 的 T-1 实施。 | @codex |
| 2026-08-12 | 完成默认拒绝策略、精确 D-6/D-7 授权、较早 Deadline 到期、尝试预算、租约 Fencing 结果提交和显式禁用生产 Executor 的首轮 TDD 循环。聚焦测试与严格 Clippy 通过；完整套件验证在最终路由运行后单独记录。T-1/T-2 保持 `In Progress`，因为重试/取消、已认证审批传输、投影、Provider 集成和持久化尚未完成。 | @codex |
| 2026-08-12 | 验证初始实施增量：`cargo fmt --all --check`、严格 all-target/all-feature Clippy 和全部 115 个测试通过。完整套件在允许的 loopback 绑定下运行，因为两个现有 Provider 超时测试无法在文件系统沙箱内绑定监听器。该证据不完成任何剩余生产边界未实施的 ADR 检查。 | @codex |
| 2026-08-12 | 用 Test-first 回归处理实施评审发现：Coordinator 现在消费规范的 D-7 派发声明，D-7 准备消耗一个 Turn 尝试槽位，Executor 失败与成功均保留效果状态证据，相同的终态审批重放在到期时间后仍保持幂等，无 Fallback 检查扫描完整生产图及 Manifest。T-1/T-2 保持 `In Progress`。 | @codex |
| 2026-08-12 | 验证评审修正：`cargo fmt --all --check`、严格 all-target/all-feature Clippy、聚焦的审批/执行/架构测试，以及完整 119 个测试的 `koduck-ai` all-target/all-feature 套件通过。未把任何生产前置条件未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-12 | 用 Test-first 回归处理第二次实施评审：受防护的终态迁移防止过期重放改写；成功与输出需要条件持久提交；不可克隆的 Turn 权威拥有尝试分配并拒绝重复 D-7 身份；不透明 Permit 防止直接 Executor 调用；带版本的规范编码现在产生固定向量的 SHA-256 动作摘要。T-1/T-2 保持 `In Progress`。 | @codex |
| 2026-08-12 | 用 Test-first 回归处理第三次实施评审：目标作用域 Profile 现在保留精确 ID/版本，Adapter 校验的 JSON 与描述符 Schema 在策略授权前 Fail Closed，可变 D-6/D-7 权威不可克隆，重建的进程内 Turn 句柄共享尝试预算与运行仲裁，Fencing 或存储故障返回 reconciliation-pending 而非未提交终态。T-1/T-2 保持 `In Progress`；跨实例权威仍归 T-3 PostgreSQL 工作。 | @codex |
| 2026-08-12 | 验证第三次评审修正：格式化、严格 all-target/all-feature Clippy、聚焦的红/绿回归，以及完整 137 个测试的 `koduck-ai` all-target/all-feature 套件通过。完整套件使用允许的 loopback 绑定，因为两个现有 Provider 超时测试无法在文件系统沙箱内绑定监听器。不支持的 JSON Schema 约束 Fail Closed，未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-12 | 验证第二次评审修正：格式化、严格 all-target/all-feature Clippy、全部 35 个聚焦 CAND-2/架构测试，以及完整 125 个测试的 `koduck-ai` all-target/all-feature 套件通过。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理第四次实施评审：显式注入的强 Turn 注册表在所有句柄丢弃后保留预算和尝试身份，并支持终态 Turn 清理；原始序列化动作输入在 JSON 解析前设上限；Executor 输出在增量构建期间限界且溢出会使完成失效；带 Fencing 的持久提交区分派发前与派发后对账。T-1/T-2 保持 `In Progress`。 | @codex |
| 2026-08-13 | 验证第四次评审修正：格式化、严格 all-target/all-feature Clippy、全部 52 个聚焦 CAND-2/架构测试，以及完整 142 个测试的 `koduck-ai` all-target/all-feature 套件通过。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理第五次实施评审：D-7 准备现在有一个公开的校验租约入口，且被 Fence 的准备不消耗尝试槽位；移除了无界的终态 Turn 清理 API；JSON 小数保留其精确的任意精度文本；对两个超过 400 行的 Source 文件重新测量了分解评审。T-1/T-2 保持 `In Progress`。 | @codex |
| 2026-08-13 | 验证第五次评审修正：格式化、严格 all-target/all-feature Clippy、全部 53 个聚焦 CAND-2/架构测试，以及完整 143 个测试的 `koduck-ai` all-target/all-feature 套件通过。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理第六次实施评审：用租约初始化的 Turn 作用域准备 Owner 替换外部可构造的多 Turn 注册表；过期绑定不能植入 Profile 身份；权威查找不再公开；该 Turn 的所有句柄保留一份共享预算/运行状态；状态生命周期由准备方加返回句柄界定了边界。T-1/T-2 保持 `In Progress`。 | @codex |
| 2026-08-13 | 验证第六次评审修正：格式化、严格 all-target/all-feature Clippy、全部 54 个聚焦 CAND-2/架构测试，以及完整 144 个测试的 `koduck-ai` all-target/all-feature 套件通过。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理第七次实施评审：C-5 在 D-6/D-7 创建前 Seal 绑定；D-6 决策需要 Typed 的同租户、同 Thread 审批 Scope；注入的运行时在多个准备方之间共享一个 Turn 权威并带弱生命周期清理；条件提交区分获胜、已存在和冲突终态；重复 JSON Schema 成员 Fail Closed；并发尝试保留其精确终态代码。已授权的无 D-6 `read_data` 路径保持可执行。T-1/T-2 保持 `In Progress`。 | @codex |
| 2026-08-13 | 验证第七次评审修正：格式化、严格 all-target/all-feature Clippy、全部 62 个聚焦 CAND-2/架构测试，以及完整 152 个测试的 `koduck-ai` all-target/all-feature 套件通过。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理第八次实施评审：C-5 绑定权威现在只来自注入的配置支撑 Sealing 服务；审批决策需要注入的 C-7 Authorizer 加独立的租户/Thread 检查；显式的强进程内权威存储防止运行时句柄和临时丢弃重置；重建的规范终态携带精确 D-7 绑定/版本，同时拒绝超大或不匹配的结果。T-1/T-2 保持 `In Progress`；PostgreSQL 权威和终态回收仍是 T-3 工作。 | @codex |
| 2026-08-13 | 验证第八次评审修正：格式化、严格 all-target/all-feature Clippy、全部 69 个聚焦 CAND-2/架构测试，以及完整 159 个测试的 `koduck-ai` all-target/all-feature 套件通过。完整套件为两个现有 Provider 超时测试使用了允许的 loopback 绑定。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用一个失败的架构回归加最小权威边界变更处理第九次实施评审：配置支撑的 C-5 Sealing 和 C-7 授权/服务构造改为 Crate 自有而非公开扩展点，每个进程运行时句柄解析到一个进程自有的 Turn 权威根，而不接受调用方构造的存储。依赖权威的测试移到 `tests/internal/` 下并仍为 Crate 单元测试，使私有生产权威不为测试便利而重新开放；AC-4 命令和受影响路径证据仅作为保持审批的路径维护而更新。T-1/T-2 保持 `In Progress`；T-2 运行时接线和 T-3 PostgreSQL 权威/终态回收仍未完成。 | @codex |
| 2026-08-13 | 验证第九次评审修正：`git diff --check`、格式化、严格 Workspace all-target/all-feature Clippy、聚焦的权威/架构与 AC-2/AC-4 命令，以及完整 159 个测试的 Workspace all-target/all-feature 套件通过。完整套件为两个现有 Provider 超时测试使用了允许的 loopback 绑定。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用三个失败回归循环处理第十次实施评审：Executor 成功和错误结果现在共享必需的提交前租约检查；自有 Schema 构造函数拒绝重复属性而非末值覆盖；全局 `OnceLock` 被替换为显式注入的 Crate 自有运行时根。进程内 Turn 状态只有在规范终态 Turn 且所有 D-7 记录均为终态、每个准备方/尝试/权威句柄都已消失后才能回收；活跃或存续权威 Fail Closed。多 Turn 查找/回收移入一个聚焦的 114 行领域子模块，使发生实质变化的聚合保持在生产 Source 例外上限以下。T-1/T-2 保持 `In Progress`；生产根/回收接线和 T-3 PostgreSQL 权威仍未完成。 | @codex |
| 2026-08-13 | 验证第十次评审修正：`git diff --check`、格式化、严格 Workspace all-target/all-feature Clippy、聚焦的 Schema/Fencing/回收/架构测试，以及完整 163 个测试的 Workspace all-target/all-feature 套件通过。完整套件为两个现有 Provider 超时测试使用了允许的 loopback 绑定。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用三个失败回归循环处理第十一次实施评审：递归的重复动作参数成员现在在规范化前 Fail Closed；无法更新本地 D-7 镜像的规范终态现在返回 Typed 对账而非捏造的失败；进程内 Turn 回收被移除，直到 T-3 能提供规范终态证明并防止预算复活。当前强权威根有意在临时句柄丢失期间保留 Turn 状态。T-1/T-2 保持 `In Progress`；T-3 持久化和安全回收仍未完成。 | @codex |
| 2026-08-13 | 验证第十一次评审修正：`git diff --check`、格式化、严格 Workspace all-target/all-feature Clippy、三个聚焦的重复输入/终态冲突/权威边界回归，以及完整 163 个测试的 Workspace all-target/all-feature 套件通过。未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用一个失败的证据一致性回归处理第十二次实施评审：T-1/T-2 证据现在区分 Source/测试接缝与未完成的生产运行时接线，移除过期的回收声明，并为 602 行 Tool 领域文件和 81 行执行 Coordinator 方法记录显式分解评审，包括已配置复杂度工具的 N/A 和替代嵌套评审。未改变任何已批准决策内容或验收状态。 | @codex |
| 2026-08-13 | 验证第十二次评审修正：`git diff --check`、格式化、严格 `koduck-ai` all-target/all-feature Clippy、聚焦的 ADR 证据一致性回归，以及完整 164 个测试的 `koduck-ai` all-target/all-feature 套件通过。架构测试文件保持在其 600 行评审阈值以下，未把任何未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用一个 Test-first 派发拒绝回归处理第十三次实施评审：规范启动拒绝现在使用非终态 `ExecutionPending::DispatchRejected` 而非未提交的 `ToolExecutionOutcome`；分解证据来自当前 Source 测量；每个 Not Started 风险覆盖行现在记录其具体缺失的验收前置条件。未改变任何已批准决策内容或验收状态。 | @codex |
| 2026-08-13 | 验证第十三次评审修正：`git diff --check`、格式化、严格 `koduck-ai` all-target/all-feature Clippy、全部 26 个聚焦执行测试、源自 Source 的 ADR 证据一致性测试，以及完整 164 个测试的 `koduck-ai` all-target/all-feature 套件通过。完整套件为两个现有 Provider 超时测试使用了允许的 loopback 绑定；未把任何生产边界未完成的验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用失败的架构回归处理第十四次实施评审：原始 D-7 派发声明迁移改为 Crate 内部，架构防护禁止将其重新公开；分解证据测试现在从当前 Source 推导每个已记录的生产/测试文件计数及 Coordinator 方法跨度，并同步了 ADR 测量值。未改变任何已批准决策内容或验收状态。 | @codex |
| 2026-08-13 | 验证第十四次评审修正：`git diff --check`、格式化、严格 `koduck-ai` all-target/all-feature Clippy、两个聚焦架构回归，以及完整 164 个测试的 `koduck-ai` all-target/all-feature 套件通过。完整套件为两个现有 Provider 超时测试使用了允许的 loopback 绑定；架构测试文件保持在其 600 行评审阈值以下，未把任何未完成验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 用 Test-first 回归处理第十五次实施评审：不可信描述符 Schema 文本在两轮反序列化之前上限为 65,536 字节，并带精确的等于/超过上限证据；唯一命名的 Crate 内部 D-7 声明/镜像迁移由全生产 Source 调用点计数防护，因此无法静默新增绕过。分解证据已同步，未改变已批准决策范围或验收状态。 | @codex |
| 2026-08-13 | 验证第十五次评审修正：`git diff --check`、格式化、严格 `koduck-ai` all-target/all-feature Clippy、聚焦的 Schema 上限/调用点/ADR 证据回归、全部 15 个审批和 26 个执行测试，以及完整 165 个测试的 `koduck-ai` all-target/all-feature 套件通过。完整套件为两个现有 Provider 超时测试使用了允许的 loopback 绑定；架构测试文件保持在其 600 行评审阈值以下，未把任何未完成验收检查提升为 `Pass`。 | @codex |
| 2026-08-13 | 实现 T-1 重试交付物并处理后续评审轮次：一个 Crate 内部的 `ToolExecutionDriver` 以恰好一次已证明的效果前重试运行 authorize、prepare、approve-or-cancel 与 dispatch（新 D-7 身份、重新评估策略、新 D-6），把预算耗尽的重试映射为 `failed/attempt_limit`，对 declined/cancelled/过期的 D-6 不经派发取消已准备的 D-7，在每次 D-6 创建与派发时重读受控时钟并把 D-7 启动时间钳制为不早于已验证的决策时间，且重试准备期间被 Fence 的所有者不送达任何已提交的旧终态。十二个聚焦重试 Fixture 覆盖该契约；已为 98 行 Driver 方法、744 行重试 Harness 和架构测试文件记录分解评审——后者现测得 675 行，高于其 600 行评审阈值，取代此前低于阈值的验证陈述。验证：格式化、严格 all-target/all-feature Clippy，以及完整 183 个测试的 `koduck-ai` 套件通过；AC-9 端到端重试验证仍是 T-2 黑盒交付物，未把任何未完成验收检查提升为 `Pass`。 | @zcode |
| 2026-08-13 | 用三个 Test-first 回归处理取消并发评审发现：中断现在消费独立提供的取消服务，因而阻塞的派发 Coordinator 不会阻止认证取消到达其 Executor 路径；权威在条件持久写入前保留活跃 D-7 终态迁移，防止过期 prepared 快照在派发后提交 `cancelled/not_started`；已接受的 D-6 也不再阻止中断关闭其尚处于 prepared 的精确 D-7。取消服务会在确认后、提交前重验租约。T-1/T-2 保持 `In Progress`；运行时组装与认证传输仍属 T-2，未提升任何验收检查。 | @codex |
| 2026-08-13 | 验证取消并发修正：`git diff --check`、格式化、严格 `koduck-ai` all-target/all-feature Clippy、15 个聚焦取消回归、ADR 证据架构回归，以及完整 198 个 `koduck-ai` all-target/all-feature 测试套件均通过。完整套件为既有 Provider 超时测试使用允许的 loopback 绑定；未提升任何尚未完成的验收检查为 `Pass`。 | @codex |
| 2026-08-13 | 用两个 Test-first 回归处理后续取消并发评审：中断获取一次权威锁内快照，因此任一存活 D-7 正在提交终态时，会在部分关闭可见同伴前返回 `ReconciliationRequired{terminal_conflict, unknown}`；运行中 D-7 的取消会在 Executor 取消副作用前原子保留其精确终态，并在取消后的 Fencing 或持久写入失败时保留该状态直到对账，因此第二个中断者不能发送重复取消。T-1/T-2 保持 `In Progress`；未提升任何验收检查。 | @codex |
| 2026-08-13 | 验证原子中断与取消后对账修正：`git diff --check`、格式化、严格 `koduck-ai` all-target/all-feature Clippy、21 个聚焦取消回归、ADR 证据架构检查，以及完整 187 个 `koduck-ai` all-target/all-feature 测试套件均通过。完整套件为两个既有 Provider 超时测试使用允许的 loopback 绑定；未提升任何尚未完成的验收检查为 `Pass`。 | @codex |
| 2026-08-13 | 验证后续取消并发修正：`git diff --check`、格式化、严格 `koduck-ai` all-target/all-feature Clippy、两个新的聚焦取消回归、ADR 证据架构检查，以及完整 185 个 `koduck-ai` all-target/all-feature 测试套件均通过。完整套件为两个既有 Provider 超时测试使用允许的 loopback 绑定；未提升任何尚未完成的验收检查为 `Pass`。 | @codex |
| 2026-08-13 | 修正取消后续变更的分解评审证据：共享终态模块为 113 物理行，85 行中断、78 行运行中取消和 86 行已保留终态提交方法现已记录其明确的内聚性与嵌套审查。架构证据检查从当前 Source 推导这些测量值。未改变任何已批准决策内容或验收状态。 | @codex |
