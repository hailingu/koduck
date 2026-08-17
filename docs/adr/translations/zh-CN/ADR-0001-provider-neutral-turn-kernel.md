# ADR-0001：Provider 中立的无工具 Turn 内核（中文翻译）

> [!IMPORTANT]
> 本文件是
> [`docs/adr/ADR-0001-provider-neutral-turn-kernel.md`](../../ADR-0001-provider-neutral-turn-kernel.md)
> 的非权威中文翻译，不是第二份 ADR，也不拥有独立的状态、审批或记录身份。
> 若中英文存在差异，以 `docs/adr/INDEX.md` 索引的英文 ADR 为准。状态、
> 子任务、验收检查、证据和链接必须与英文版同步更新。

## 元数据 [Required]

- **决策状态**：Accepted
- **实施状态**：Complete
- **日期**：2026-08-11
- **作者**：@codex
- **决策负责人**：@linhai
- **所需审批人**：@linhai
- **记录范围**：Project
- **审批人 [Conditionally Required — Decision Status 为或曾为 `Accepted`]**：@linhai
- **审批时间 [Conditionally Required — Decision Status 为或曾为 `Accepted`]**：2026-08-17T08:57:39Z
- **审批证据 [Conditionally Required — Decision Status 为或曾为 `Accepted`]**：Approve
- **拒绝执行人 [Conditionally Required — Decision Status 为 `Rejected`]**：N/A — Decision Status 为 `Accepted`
- **拒绝时间 [Conditionally Required — Decision Status 为 `Rejected`]**：N/A — Decision Status 为 `Accepted`
- **拒绝证据 [Conditionally Required — Decision Status 为 `Rejected`]**：N/A — Decision Status 为 `Accepted`
- **退役执行人 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`
- **退役时间 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`
- **退役证据 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`
- **退役原因 [Conditionally Required — Decision Status 为 `Deprecated` 或 `Superseded`]**：N/A — Decision Status 为 `Accepted`
- **阻塞前状态 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `Complete`
- **阻塞项与证据 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `Complete`
- **阻塞项负责人 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `Complete`
- **阻塞退出或复查条件 [Conditionally Required — Implementation Status 为 `Blocked`]**：N/A — Implementation Status 为 `Complete`
- **相关资料 [Optional]**：[英文权威 ADR](../../ADR-0001-provider-neutral-turn-kernel.md)；[Koduck Trello 卡片 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87)
- **架构来源 [Conditionally Required — 产品需求]**：`docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-1
- **取代 [Conditionally Required — 本 ADR 替换其他 ADR]**：None
- **被取代 [Conditionally Required — 本 ADR 被替换]**：None

## 要求级别图例 [Required]

- **`[Required]`**：章节或字段始终适用，必须保留并提供完整、可验证的内容。只有模板明确允许空结果时，才可使用 `None — <原因>`，不得留空。
- **`[Conditionally Required — <触发条件>]`**：触发条件成立时必须完成；不成立时保留 `N/A — <原因>`，除非模板明确要求删除或作为未来生命周期说明保留。未评估触发条件即为内容不完整。
- **`[Optional]`**：删除后不影响审批、实施、完成或验证；如保留则必须准确完整，且不能替代必填证据。

`[Required]` 章节内未单独标注的字段均为必填。

## 背景与问题陈述 [Required]

Koduck 仓库目前还没有服务实现。其前身曾把 REST/SSE 展示、认证、Provider
选择、编排、持久化、工具使用和后台任务都放在一个 Rust 服务中，但对应
基础设施已经移除。因此前身只作为功能调研证据，不是运行、Wire Contract、
存储、部署或回滚基线。

`docs/architecture/ADD-0001-ai-service-codex-alignment.md` 定义了目标边界，
并把 CAND-1 选为第一个可执行切片。本 ADR 必须决定如何让单个已认证、
无工具 Turn 通过新的自有 v1 `POST /api/v1/ai/chat` 和
`POST /api/v1/ai/chat/stream` 接口执行，同时引入自有 Thread、Turn、Item
类型、唯一的 Provider 中立编排 Owner、先持久化后发布的历史，以及带
Fencing 的前台活性机制。它定义 AI 自有的持久化 Thread/Turn/Item 基线，
不承担旧兼容、共享历史、前身制品、回切或运行时 Fallback 要求。

## 范围 [Required]

范围内：

- 新建 Rust `koduck-ai` 服务 Crate，并加入根 Cargo Workspace。
- 为单个前台无工具 Turn 定义自有 Thread、Turn、Item、终态结果、Trust
  Context、Lease Generation、Provider Event 和领域错误类型。
- 一个编排状态机，以及由 Consumer 拥有的 Provider 和 History Port。
- 一个 OpenAI-compatible Provider Adapter；通过确定性协议测试服务器验证，
  不访问真实网络。
- 实现新的已认证同步 Chat、SSE Chat 和 Interrupt v1 契约，范围仅限纯文本、
  无附件、无工具切片。
- 一个 AI 自有 PostgreSQL History Adapter，支持初始接受、先持久化后发布、有序重放、
  条件终态 Append、Lease Acquire/Renew/Fence 和孤儿 Turn 对账。
- 本记录验收检查要求的契约、状态机、持久化故障、崩溃、租约和无 Fallback 测试。

范围外：

- 高权限工具、MCP 调用、审批、Sandbox、扩展、Agent Profile、Skill、
  Plugin、后台任务、Fork 和 Checkpoint。
- Semantic Memory、后台 Multitask、Fork、Checkpoint，以及留给 CAND-3 的
  扩展幂等模型。
- 一个以上的生产 Provider 协议、Provider Fallback，或超出自有 REST/SSE v1
  契约的公共 Typed Protocol。
- 附件、图片输入、Memory Ranking、Ask/Clarification Flow、Task API、
  主动式多 Agent 执行、Web/原生 UI、部署和流量切换。

## 张力、约束与开放问题 [Required]

### 已识别张力 [Conditionally Required — 存在相互竞争的目标或权衡]

| ID | 张力 | 影响 | 决策 |
| --- | --- | --- | --- |
| TN-1 | 可见前持久化会增加延迟，并让 Stream 可用性依赖 History Path。 | Append 前发布会产生无法由权威重放复现的客户端可见 Item；无界等待会让 Turn 无限停滞。 | 每次 Append 必须在发布前完成，Deadline 为 2 秒，并限制未发布 Buffer；不得发布未提交 Item，而是暴露 Typed 持久化故障。 |
| TN-2 | 多 Crate 架构能强化边界，但首个切片只有一个具体 Consumer 和 Service。 | 过早拆 Crate 会增加 Build/Versioning 成本；一个无差别模块又会重现前身耦合。 | 先使用一个 Service Crate，内部按 Domain、Application、Adapter 组织并强制向内依赖；只有后续 Accepted 决策证明存在第二个 Consumer 或独立生命周期时才拆 Crate。 |
| TN-3 | 快速检测 Owner 丢失可缩短恢复时间，但短租约会增加暂停或分区时误 Fencing 的概率。 | 误 Fence 会取消有效工作；长 Lease 会让孤儿 Turn 长时间显示为 Active。 | 每 5 秒续租，Lease 为 20 秒，对账前额外允许 2 秒时钟偏差；绝不把同一个 Turn 转移给新 Owner。 |

### 约束 [Required]

- 权威架构来源是 `docs/architecture/ADD-0001-ai-service-codex-alignment.md`
  CAND-1；本 ADR 只能细化该切片，不得扩大范围。
- `@linhai` 于 `2026-08-11T10:37:34+08:00` 重新批准 Greenfield 修订后，架构
  来源已恢复为 `Current`，且双方 CAND-1 内容一致。这满足架构来源前置条件，
  但不批准这份独立治理的 ADR。
- 新 Identity/Trust Context 契约权威。Presentation Adapter 从配置的
  Gateway/Auth 边界接收已验证且不可变的 Trust Context；Core 不解析或验证
  Bearer Token。
- Provider Wire Type、Axum Request/Response Type、Persistence Record 和
  Service Client Type 不得进入 Domain 或 Application Module。
- Terminal Turn 不得重新 Active。Resume 必须从持久化有序历史中，在同一
  Thread 上创建新的 Turn。
- 每个外部可见 Item 或 Terminal Outcome 必须在发布 REST Response 或 SSE
  Event 前完成持久化 Append。
- Presentation Boundary 确认接受前，初始 Turn、Input Item 和 Lease
  Generation 必须全部持久化。初始写失败时返回错误，且不得暴露已接受 Turn。
- 初始接受与 Append 会预先分配稳定的 Item Identity。若 2 秒 Attempt Deadline
  在 Commit Acknowledgement 阶段到期，PostgreSQL Transaction Advisory Lock
  按该 Identity 串行化结果对账。Reconciliation 是另一个最多 2 秒的 PostgreSQL
  Attempt：找到 Durable Result 时返回该结果，确认不存在时报告 Unavailable；若仍
  无法确定，同样返回 Unavailable，并在任何 Durable Write 中保留稳定 Identity。
  两个顺序 Attempt 的总等待因此最多为 4 秒。
- 每个 Turn 的未发布 Buffer 上限为 64 Items 或 1 MiB 序列化 Item Payload，
  以先达到者为准。每次 Append Deadline 为 2 秒。达到上限或 Deadline 时停止
  消费 Provider，不发布未提交 Item，并在 Live REST/SSE Response 中返回
  `durability-unavailable`。
- Resume Provider Context 独立限制为 4096 个 Prior Items 或 1 MiB 规范序列化
  Item Payload。PostgreSQL Query 最多读取 4097 个有序 Row；越界时返回自有
  `400 invalid-request`，且绝不静默截断 Durable History。
- 所有生产 PostgreSQL Operation 均使用同一个 2 秒 Attempt Deadline。Lease-renewal
  与 Failed-append Recovery 在每个生产 History Instance 中共享最多 256 个
  Background Worker；Connection 与 Migration Startup Attempt 使用同一 Deadline。
  Append Outage 会停止 Renewal Worker，并把该 Worker 的 Permit 原子移动到
  Recovery Worker；两个 Owner 之间不会降低 Shared Admission 计数，因此即使满载，
  Recovery 也持续持有已保留容量。该 Handoff 等待受 Renewal Database Attempt 的
  2 秒 Deadline 约束；其他饱和情况以
  `durability-unavailable` Fail Closed。
- Failed-append Recovery 只有一个总计 22 秒的 Window。每次 Attempt 前计算剩余
  Window，并把 Database Attempt 限制为 2 秒与剩余时长中的较小值；Window
  耗尽后不再启动 Attempt，而是交由带 Fencing 的 Reconciliation。
- Provider Connection Deadline 为 5 秒，Response Header 与 Stream Idle Deadline
  均为 30 秒，Total Response Processing Deadline 为 120 秒。超时产生 Provider
  Error，并通过正常 Terminal Arbitration 关闭已接受 Turn。
- 前台 Owner 每 5 秒 Renew Lease。Lease 在最后一次持久化续租后 20 秒过期；
  额外经过 2 秒时钟偏差 Margin 后才能对账。只有当前 Generation 可以 Append。
- 并发 Reconciler 使用由 Thread ID、Turn ID 和 Lease Generation 组成的
  Conditional Key。只能有一个 Reconciler Append 孤儿终态 `cancelled`；旧
  Owner 和失败的 Reconciler 接收 Typed Fenced Result。
- Source 和 Test 必须遵循 `docs/development/software-engineering-standard.md`
  与 `docs/development/rust-standard.md`。本 Draft 不授权任何工程例外。
- 在本 ADR `Accepted` 前，不得开始实施、Build、部署或其他运维工作。任何
  Build 或部署还必须按仓库策略取得自己的 Accepted 运维授权。

### 开放问题 [Conditionally Required — 存在或起草期间解决过重大问题]

| ID | 问题 | Owner | 截止日期 | 状态 | 结论与证据 |
| --- | --- | --- | --- | --- | --- |
| Q-1 | 哪个契约定义 CAND-1 的 Request、Response、SSE Event、Header、Status Code 和 Interrupt 行为？ | @linhai | 2026-08-12 | Resolved | 新实现权威。本 ADR 的详细设计定义 REST/SSE v1；`koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` 是实施副本及 Golden Fixture。前身 Commit `c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe` 只能提供功能场景，不产生 Parity 要求。结论来自 2026-08-11 当前 Codex 任务中的仓库 Owner 指示。 |
| Q-2 | 前身基础设施已移除时，适用什么回滚或 Fallback 目标？ | @linhai | 2026-08-12 | Resolved | CAND-1 不适用：这是部署前的 Greenfield 实施决策；失败候选项不晋级，并回退 Source 或隔离 Artifact。首次部署需要 Accepted OCR；存在已验证新 Artifact 后，后续 OCR 只能选择已验证的新 Artifact。无需前身 Artifact、APISIX 旧 Route、Route-back 或运行时 Fallback。结论来自 2026-08-11 当前 Codex 任务中的仓库 Owner 指示。 |
| Q-3 | 谁拥有初始接受、有序 Append/Replay、条件终态 Append 和带 Fencing Lease Generation？ | @linhai | 2026-08-12 | Resolved | Consumer-owned `TurnHistory` Port 与 AI 自有 PostgreSQL Adapter 权威拥有 Thread/Turn/Item 和前台 Lease。Memory 留给后续 Semantic Memory 集成，Multitask 留给后续后台任务集成；二者都不是 CAND-1 History 依赖。结论来自 2026-08-11 当前 Codex 任务中的仓库 Owner 指示。 |

## 决策驱动因素 [Required]

1. **确定性生命周期所有权**：一个 Application 状态机必须拥有有效 Transition，
   并区分完成、失败、已认证 Interrupt、Dependency Cancellation、持久化故障和
   Owner 丢失。
2. **重放一致性**：客户端不得看到权威 History Adapter 无法按相同顺序重放的
   Domain Item。
3. **自有契约**：有界 REST/SSE v1 切片必须精确符合本 ADR 定义的新 Request、
   Response、Header、Event、Status 和 Interrupt 行为。
4. **可替换边界**：Provider、Presentation 和 History 细节必须作为 Adapter
   位于 Consumer-owned Port 外部。
5. **Greenfield 隔离**：实施不得依赖已移除的前身 Artifact、Route、共享历史
   或运行时 Fallback。
6. **故障隔离**：Process Crash、Store Outage、Lease Expiry 和并发 Reconcile
   必须有精确终态与 Fencing 结果。

## 考虑过的方案 [Required]

### 方案 A：一个模块化 Rust Service Crate，使用自有 Port 与 Adapter

新建 `koduck-ai` Crate，由其 Domain 与 Application Module 拥有 Lifecycle
和 Port Contract。Axum、OpenAI-compatible Provider 与最小 History 实现保留
为 Adapter。通过测试模块依赖方向和可观察契约，不为每个边界创建独立 Crate。

优点：

- 以最小的初始 Workspace 和 Dependency Surface 建立所需所有权和测试接缝。
- 后续可由单独论证的 Consumer 或 Lifecycle Boundary 进行拆分，而不让
  Framework/Wire Type 泄漏进 Core。

缺点：

- Rust Crate Boundary 无法机械阻止所有内部 Import，因此必须通过 Architecture
  Test 和 Review 强制允许的 Module Dependency。
- 未来多 Service 或可复用 Library 拆分可能移动 Module。

### 方案 B：立即把 Domain、Core、Protocol、Provider 和 History 拆成独立 Crate

在首个可执行 Turn 前创建由多个窄职责 Crate 组成的 Workspace。

优点：

- Cargo Dependency 能机械暴露非法跨边界 Import。
- 从第一个 Revision 起即可拥有可独立复用的 Package。

缺点：

- 当前尚无第二个 Consumer 或独立 Release Lifecycle。
- 在证明所需变化前，就会提交更多 Manifest、Public API 和 Integration Wiring。

### 方案 C：先移植前身 Chat Flow，之后再提取边界

把现有 Handler/Provider/Persistence Path 复制或改造进新仓库，保留行为，
以后再处理 Domain Ownership。

优点：

- 复用最多已知行为，可能较早得到 Endpoint。
- 减少初始 Contract Translation 工作。

缺点：

- 保留 CAND-1 本应消除的耦合与分散状态 Transition Ownership。
- 使先持久化后发布与 Generation Fencing 成为跨模块 Retrofit，而不是 Core
  Invariant。

## 决策 [Required]

**选择方案**：方案 A — 一个模块化 Rust Service Crate，使用自有 Port 与
Adapter。

**理由**：CAND-1 需要独立的 Transport、Provider、History 和 Orchestration
故障边界，但目前只有一个 Service 和一个初始 Consumer。面向 Module 的 Service
Crate 可以建立向内依赖方向与 Consumer-owned Trait，而无需过早跨 Package
公开内部类型。它还能为 Contract/Fault Test 提供确定性进程内 Adapter。方案 B
在证明变化前增加 Public/Package Surface；方案 C 则保留本任务必须移除的耦合。

### 后果 [Required]

正面：

- Thread、Turn、Item 和 Terminal State Invariant 由一个 Owner 持有，不依赖
  REST/SSE、Provider 或 Persistence Representation。
- Presentation Contract、Provider Protocol 与 History 行为可独立测试和替换。
- 先持久化后发布与 Lease Generation Fencing 成为 Application Invariant，
  而不是 Handler 约定。
- 首次实施保持足够窄，CAND-2 至 CAND-5 仍可作为独立决策。

负面：

- CAND-1 History Adapter 刻意只覆盖前台 Turn；CAND-3 扩展 Lineage、Checkpoint、
  Semantic Memory Projection 与后台任务集成，但不转移权威所有权。
- 持久化延迟成为响应延迟的一部分；Store Outage 会让 Stream 停在持久化前缀。
- 在后续决策证明拆 Crate 合理前，Module Dependency Rule 需要专门的
  Architecture Test 和 Review。

缓解：

- 所有外部 Representation 保持在 Adapter 内；权威 History Surface 只通过
  CAND-1 Consumer-owned Port 定义。
- 强制 2 秒 Append Deadline 和未发布 Buffer 上限，并在确定性测试中捕获
  Latency/Failure 证据。
- 增加 Dependency Direction Test，拒绝 Domain/Application Source Path 中的
  Axum、Provider Wire 和 Persistence Import。

### 详细设计 [Required]

服务使用如下向内依赖方向：

```text
REST/SSE adapter ─┐
Provider adapter ─┼─> application turn runner ─> domain lifecycle and values
PostgreSQL adapter ─┘           │
                                └─> consumer-owned provider/history ports
```

Domain Lifecycle 如下：

```text
started ──> completed
   │      ├> failed
   │      ├> interrupted
   │      └> cancelled
   └──────> recovery-pending ──> failed
                          └────> cancelled
```

`completed`、`failed`、`interrupted` 和 `cancelled` 均为终态。Provider Error
进入 `failed`；已认证 Client Stop 进入 `interrupted`；Platform/Dependency Stop
或被 Fence 的 Owner 丢失进入 `cancelled`。Turn 接受后 History Append 失败，
Live Owner 进入 `recovery-pending`、停止消费 Provider，并只暴露持久化前缀和
Transport Diagnostic `durability-unavailable`——即下文 Wire Contract 定义的
带内 SSE `error` Event 或同步 `503` Problem。History 恢复后，由一次 Conditional
Terminal Append 将其关闭为 `failed`；如果 Owner 已过期并被 Fence，则由
Reconciliation 关闭为 `cancelled`。

Application Layer 消费两个 Port：

- `ModelProvider`：接收自有 Model Input，发出有序自有 Delta、Usage、Completion
  或 Typed Provider Error。首个 Adapter 转换 OpenAI-compatible Chat Completions
  Stream；任何 Provider JSON Type 均不得跨越 Adapter Boundary。
- `TurnHistory`：原子接受初始 Turn/Input/Lease State；在 Expected Generation
  下 Append Item；读取有序 History；Renew 当前 Generation；有条件地 Fence
  过期 Generation；并在 Thread/Turn/Generation Key 下有条件 Append 一个终态。

首个 `TurnHistory` Adapter 使用 PostgreSQL 作为共享持久状态。它拥有四个逻辑
Relation：租户范围 Thread 身份 `threads`、Lifecycle 与下一个 Sequence 的
`turns`、以 `(turn_id, sequence)` 为 Key 的 Append-only `turn_items`，以及当前
Generation/持久化 Expiry 的 `turn_leases`。初始 Thread/Turn/Input/Lease 在同一
Transaction 提交。每次 Append 在 Expected Lease Generation 下 Lock 或条件更新
Turn，只分配一个 Next Sequence，并在发布前提交。每个 Turn 只能有一个终态；
Fence/Reconcile 同时比较 Generation 与 Expiry。每个读写 Predicate 都包含 Tenant
和 Thread Ownership。CAND-1 不得把进程内 Store、Memory Client、Multitask
Client、前身服务或其他 Adapter 作为运行时 Fallback。

REST/SSE v1 权威 Wire Contract 为：

- `POST /api/v1/ai/chat` 接受可选 UUID `thread_id` 与不超过 65,536 Bytes 的非空
  UTF-8 `input` JSON，`Content-Type: application/json`；未知 Field 被拒绝。成功
  返回 `200`；JSON 恰好包含 `thread_id`、
  `turn_id`、`status`、`items`、`usage`；`status` 为 `completed`；每个 Item 包含
  且仅包含 `item_id`、正整数 `sequence`、`type: agent_message_delta` 和非空字符串
  `content`。`usage` 恰好包含非负整数 `input_tokens`、`output_tokens`、
  `total_tokens`，且 `total_tokens = input_tokens + output_tokens`。
- `POST /api/v1/ai/chat/stream` 接受同一 JSON，返回 `200` 与
  `Content-Type: text/event-stream`。Event Name 为 `turn.started`、
  `item.created`、恰好一个 `turn.completed`、`turn.failed`、
  `turn.interrupted` 或 `turn.cancelled`，以及 Transport Diagnostic
  `error`。每个 `turn.*` 或 `item.created` Event JSON 包含一致的
  `thread_id`、`turn_id` 和严格递增正整数 `sequence`。`turn.started` Data 恰好
  包含 `thread_id`、`turn_id`、`sequence`、`status: started`；`item.created`
  另外恰好包含 `item_id`、`type: agent_message_delta`、非空 `content`；Terminal
  Data 恰好包含 `thread_id`、`turn_id`、`sequence` 和匹配的 `status`，且仅
  `turn.completed` 额外包含上述精确 `usage` Object。`turn.started` 之后发生的、
  使 Durable Terminal Append 无法完成的失败，改为最多发出一个 `error` Event，
  其 Data 为下文定义的精确 Problem Body（中途持久化故障的 Code 为
  `durability-unavailable`），随后在不发出 Terminal Event 的情况下关闭 Stream；
  该 Turn 随后由有界 Recovery 或 Fencing Reconciliation 关闭为 `failed` 或
  `cancelled`。`error` Event 不携带 `thread_id`、`turn_id` 或 `sequence`，且
  Terminal Event 发布后绝不再发出 `error` Event。
- `POST /api/v1/ai/turns/{turn_id}/interrupt` 在调用者 Trust Context 拥有 Active
  Turn 时返回 `202`，Body 恰好包含 `turn_id` 与
  `status: interrupt-requested`，无其他 Field；Request 无 Body。Stream 后续发出唯一持久化
  `turn.interrupted`。未知或非 Owner Turn 都返回 `404` 且不泄露差异；已终态
  Turn 返回 `409` 与 Error Code `turn-already-terminal`。
- 缺失或无效 Identity 返回 `401`、`WWW-Authenticate: Bearer`，并用
  `application/problem+json` 返回 `invalid-identity`。无效 JSON/Input 返回
  `400`；初始或中途持久化失败返回 `503` 与 `durability-unavailable`；在已开始
  的 SSE Stream 上，该诊断改为以带内 `error` Event 投递，而非 HTTP Status。每个 Error
  Body 恰好包含 `type: about:blank`、`title`、数值 `status`、稳定 `code` 和 UUID
  `correlation_id`；`title` 是把 Code 从 Kebab Case 转成单词并把首字母大写。
  初始 Transaction 失败时，错误响应不得暴露 Accepted Turn。
- 若 Resume 的有序 Provider Context 超过 4096 Items 或 1 MiB 规范序列化 Item
  Payload，则返回自有 `400 invalid-request`；不得创建新 Turn/Provider Request，
  也不得截断或修改此前 Durable History。

同步 Chat 只 Buffer 已持久化 Item 后返回；SSE 只有在对应 Item/Terminal Append
成功后才发布 Event。Resume 加载之前的持久化历史，在同一 Thread 上创建不同
Turn ID，且不修改此前 Terminal Turn。契约副本和 Golden Fixture 只是实施证据，
不是第二权威来源。

## 实施计划 [Required]

**完整任务结果**：一个 Provider 中立的 Thread/Turn/Item 内核，通过
OpenAI-compatible Provider Path 让单个已认证、纯文本、无工具 Turn 经两个
自有 REST/SSE v1 Route 执行；确定性证据证明有序持久化重放，以及本文定义的精确
完成、Provider Failure、已认证 Interrupt、Durability Outage、Crash、Lease
Expiry、Stale Owner、Concurrent Reconciler 和无旧 Fallback 结果。

允许的子任务状态：`Not Started`、`In Progress`、`Blocked`、`Complete` 或
`N/A — <具体原因>`。

| ID | 目标或交付物 | 包含范围 | 状态 | 实际实施证据 |
| --- | --- | --- | --- | --- |
| T-1 | 创建仓库 Scope Routing、自有 Domain Lifecycle、Application Turn Runner、Consumer-owned Port 和一个 OpenAI-compatible Provider Adapter。 | 根 `AGENTS.md` 中 `koduck-ai/**` Scope Routing 行；根 Cargo Workspace；`koduck-ai` Domain、Application、Provider Adapter、Typed Error、Unit Test 和 Dependency Direction Test。 | Complete | Commit `af10ac9` 实现自有 Domain/Application 边界、Runner、Provider Port 和确定性 OpenAI-compatible Protocol Adapter；Review Correction Commit `56073a0`、`df49b69`、`11b5ea2`、`fe3beb9`、`a7258bc`、`a7b6faa` 与 `31ef43f` 将 Accept 后 Provider Failure 持久化，惰性消费 Frame，以结构化方式解码 Nullable Usage，在 Response Header 到达前提供有界 Idle Poll，把同一历史 Turn 的流式 Delta 合并为一条 Assistant Message，在 Consumer Stream 被丢弃时取消 Provider Request/Response Work，拒绝超过 1 MiB 的未终止 Provider Frame，按规范序列化结果计算 Payload Bytes，让已接受的 Interrupt 优先于每一种 Provider Terminal，并使 Runner 遵循实际持久化的终态。AC-1 至 AC-3、AC-14 通过。 |
| T-2 | 实现自有已认证 REST/SSE v1 契约并冻结 Golden Fixture。 | 纯文本无工具 `POST /api/v1/ai/chat`、`POST /api/v1/ai/chat/stream` 与 Interrupt Route；Trust Context Handoff；Request/Response/Header/Status/SSE Fixture Hash；Contract Test。 | Complete | Commit `4a7bf5d` 实现 Framework-neutral REST/SSE/Interrupt Adapter、Resume/Interruption、Contract Copy 与三个带 Hash 的 Fixture；Review Correction Commit `56073a0`、`df49b69`、`11b5ea2`、`fe3beb9`、`a7258bc`、`31ef43f` 与 `d444cf3` 移除 Request-wide Serialization，增量发送 Durable Event，在终态前报告 Post-start Failure，在终态已发送后即使 Replay 失败也正常关闭，在 Provider Idle 或仍等待 Response Header 时支持并发 Interrupt，把同步 Failed Turn 映射为 `503`，严格校验 UTF-8 与完整 JSON Escape，把超大 Body 与不支持的 Method 路由到自有 Problem Response，拒绝非 HTTPS Provider Endpoint，并为 Runtime Failure Problem Body 加入 UUID Correlation ID。AC-4 至 AC-7、AC-13 通过。 |
| T-3 | 实现 AI 自有 PostgreSQL History 与带 Fencing Liveness Adapter，并证明故障、恢复与无 Fallback 行为。 | 初始持久化接受、Append/Replay、Migration、Deadline/Buffer Cap、Lease Acquire/Renew/Fence、Orphan Reconciliation、Crash/Fault Test 和无旧运行依赖证据。 | Complete | Commit `46f2a39` 与 `80fc2ff` 实现 Fail-closed Policy、Schema/Adapter Boundary、精确 Lease Timing 与 Crash/Race Evidence。Commit `08cc1b3` 实现带完整 Tenant Key 的 SQLx Executor、幂等 PostgreSQL Migration、Reqwest Provider Transport、Axum Route、经验证的 Runtime Configuration Schema 与 Executable Entry Point。Review Correction Commit `56073a0`、`df49b69`、`11b5ea2`、`fe3beb9`、`a7258bc`、`a7b6faa` 与 `d444cf3` 强制 Append Deadline、Serialized-payload Cap 和执行期 64-Item Cap，运行 Renewal/Reconciliation Worker，重试临时 Heartbeat，持久化 Subject Ownership，保留有界 Recovery Ownership，使并发 Thread History 中每个 Turn 在 Provider Context 内保持连续，避免 Request Shutdown 同步等待卡死的 Renewal，并在单次有界 Append Operation 内、PostgreSQL Turn Row Lock 下仲裁每一种 Provider Terminal 与 Interrupt。AC-8 至 AC-12 均通过。 |

**受影响路径**：`README.md`；`AGENTS.md`；`Cargo.toml`；`Cargo.lock`；
`koduck-ai/Cargo.toml`；`koduck-ai/src/lib.rs`；
`koduck-ai/src/adapters/mod.rs`；
`koduck-ai/src/domain/**`；`koduck-ai/src/application/**`；
`koduck-ai/src/adapters/http/**`；`koduck-ai/src/adapters/provider/**`；
`koduck-ai/src/adapters/history/**`；`koduck-ai/src/main.rs`；
`koduck-ai/migrations/**`；`koduck-ai/tests/**`；
`koduck-ai/docs/runtime-configuration.md`；
`koduck-ai/docs/contracts/cand-1-rest-sse-v1.md`；
`docs/adr/ADR-0001-provider-neutral-turn-kernel.md`；
`docs/adr/translations/zh-CN/ADR-0001-provider-neutral-turn-kernel.md`；
`docs/adr/INDEX.md`；`docs/architecture/ADD-0001-ai-service-codex-alignment.md`；
以及 `docs/architecture/translations/zh-CN/ADD-0001-ai-service-codex-alignment.md`。

**迁移与回滚策略 [Conditionally Required — 替换或改变现有行为]**：N/A — 这是
Greenfield Source 决策，不存在现有 Koduck AI Runtime、前身 Artifact、APISIX
旧 Route、共享 History 或 Fallback Path。任何未通过验收的候选项不得晋级，可在
保留证据后回退或隔离。本 ADR 不授权部署。首次部署需要 Accepted OCR，并记录
不可变新 Artifact 与恢复流程；存在已验证新 Artifact 后，后续回滚只能在
Accepted OCR 下选择已验证的新 Artifact。

### 工程例外 [Conditionally Required — 超出或豁免工程规则]

N/A — 所提设计不超出或豁免仓库工程规则。实施期间发现的任何例外均属于使审批
失效的变更，必须先加入本节，才能继续受影响的 Source Change。

## 契约到检查追踪 [Required]

| Clause ID | 规范契约条款 | 验收检查或确定性测试 |
| --- | --- | --- |
| CT-1 | Domain/Application Dependency 向内，Provider/Persistence Type 不越过 Application Boundary。 | AC-1 |
| CT-2 | 正常 Provider Stream 生成一个有序 Completed Turn，且仅发布 Durable Item。 | AC-2、AC-5 |
| CT-3 | Provider Failure 生成 `failed` 而非 `completed`；同步失败映射为 `503 provider-unavailable`。 | AC-3、AC-15 |
| CT-4 | REST/SSE 接受大小写不敏感、带标准参数的 `application/json`，并拒绝其他 Media Type。 | AC-4、AC-5、AC-15 |
| CT-5 | 同步 Interrupted/Cancelled 分别映射为 `409 turn-interrupted` 与 `409 turn-cancelled`。 | AC-15 |
| CT-6 | Resume 在同一 Thread 创建新 Turn，不修改此前 Terminal History。 | AC-6 |
| CT-7 | 已认证 Interrupt 仅在 `started` Turn 仍有未 Fenced、未过期 Live Owner 时赢得 Terminal Arbitration；Interrupt Transaction 锁定 Ownership、Append 唯一 Durable `interrupted` Item，并在返回 Accepted 前提交 Terminal Status。已脱离 Stream 的 `recovery-pending` 与 Expired Turn 拒绝 Interrupt。Unknown/Non-owned 仍不可区分，Cancellation 保持独立。 | AC-7；`postgres_subject_ownership::production_postgres_contract` |
| CT-8 | 初始或中途 Durability Outage（包括 Accepted Control-state Read Outage）不发布未提交状态，并返回 `durability-unavailable`；SSE Stream 开始后，该诊断以最多一个带内 `error` Event 投递，Data 为精确 Problem Body，Stream 随后在不发出 Terminal Event 的情况下关闭，且 Terminal Event 发布后绝不再发出 `error` Event。 | AC-8；`control_read_outage_enters_failed_recovery_handoff`；`runtime_wiring::mid_turn_failure_is_reported_inside_an_started_sse_stream`；`sse_terminal_consistency::replay_failure_after_sse_terminal_does_not_emit_error_event` |
| CT-9 | Unpublished Data 上限为 64 Items/1 MiB，所有 PostgreSQL Attempt 上限为 2 秒。 | AC-9、AC-16 |
| CT-10 | Lease Renewal/Expiry/Skew/Fencing/Reconciliation 最多生成一个 Durable Orphan Terminal。 | AC-10、AC-11 |
| CT-11 | Renewal 与 Recovery 共享 256-Worker Admission Bound；失败 Turn 把 Renewal Permit 直接移动到 Recovery，不释放 Reservation，使满载交接保持原子性。 | AC-16；`append_outage_confirms_admission_handoff_before_recovery`；`renewal_recovery_handoff_retains_permit_reservation` |
| CT-12 | Provider Connect/Header/Idle/Total Deadline 分别为 5/30/30/120 秒。 | AC-17 |
| CT-13 | 无 Validated Trust Context 的 Request 在 Application/Provider/History 前终止。 | AC-13 |
| CT-14 | CAND-1 仅有一个 PostgreSQL History，且无前身、Memory 或 Multitask Fallback。 | AC-12 |
| CT-15 | 根 Scope Routing 治理所有维护型 `koduck-ai/**` Source/Configuration。 | AC-14 |
| CT-16 | 初始接受或 Append 的 Commit Acknowledgement 超时或失败后，Adapter 启动一个最多 2 秒的独立稳定 Item Identity Reconciliation Attempt；找到 Durable Result 时返回该结果，确认不存在或对账仍不确定时返回 `Unavailable`，Operation 与 Reconciliation 总等待最多 4 秒。 | AC-8、AC-9；`timed_out_commit_returns_the_reconciled_durable_outcome`；`timed_out_commit_reports_unavailable_only_after_absence_is_reconciled`；`failed_commit_acknowledgement_returns_the_reconciled_durable_outcome`；`commit_reconciliation_attempt_is_bounded_by_the_database_deadline`；`reconciliation_waits_for_the_matching_writer_identity` |
| CT-17 | Resume Provider Context 上限为 4096 个 Prior Items 与 1 MiB 规范序列化 Payload；越界 History 返回自有 `400 invalid-request`，且不截断或修改 History。 | AC-6、AC-15；`aggregate_history_over_one_mib_is_rejected_before_provider_construction`；`aggregate_history_over_four_thousand_ninety_six_items_is_rejected`；`context_limit_maps_to_the_owned_invalid_request_service_error`；`oversized_resume_context_uses_the_owned_invalid_request_problem` |

## 风险覆盖矩阵 [Required]

| Risk Dimension | 适用性与场景 | Owner Boundary | 确定性验证 | 精确预期结果 | Checks | 状态与稳定证据 |
| --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | 适用 — 并发 Terminal Writer/Reconciler 争抢同一 Generation。 | Application Arbitration 与 PostgreSQL History | AC-5、AC-7、AC-10、AC-11 | Visible Item 先 Durable；唯一 Terminal 胜出；旧 Writer 被 Fence。 | AC-5、AC-7、AC-10、AC-11 | Pass — Contract、Terminal Arbitration、Liveness Test。 |
| Timeout and deadline | 适用 — Database Startup/Query/Commit Acknowledgement/Reconciliation 或 Provider Establishment/Streaming 卡死。 | Runtime Assembly、SQLx 与 Provider Adapter | AC-9、AC-16、AC-17 及 Commit-reconciliation Unit Test | 每个 Database Attempt 在 2 秒停止；Commit Acknowledgement 失败/超时后执行一个最多 2 秒的独立稳定 Identity Reconciliation，因此两个顺序 Attempt 最多等待 4 秒，对账仍不确定时返回 `Unavailable`；Provider 在 5/30/30/120 秒停止并生成 Typed Terminal Failure。 | AC-9、AC-16、AC-17 | Pass — Startup/Query Deadline、Bounded Commit-reconciliation Test 与 Architecture Regression。 |
| Cancellation and interruption | 适用 — Interrupt 与 Provider Terminal、Failed-append Recovery、Lease Expiry 或 Downstream Disconnect 竞争。 | Runner、PostgreSQL History 与 HTTP/SSE Adapter | AC-7、AC-15 | 返回 202 表示 Live-owner Interrupt 已是唯一 Durable `interrupted` Terminal，Provider/Recovery Competitor 只回放该终态；Recovery-pending 或 Expired Owner 拒绝 Interrupt；Dependency/Disconnect 仍为 `cancelled`；同步 409 Code 可区分。 | AC-7、AC-15 | Pass — Interrupt、Transactional Arbitration、生产 PostgreSQL Recovery-pending/Expired-owner Rejection 与 Sync Mapping Regression。 |
| Resource bounds and backpressure | 适用 — Provider Flood、Resume History 无界增长或 Active Turn 耗尽后台容量。 | Provider、Durability Policy、PostgreSQL History | AC-6、AC-9、AC-15、AC-16 及 Aggregate-history Regression | Item/Payload Cap Fail Closed；Resume 最多读取 4097 Rows，超过 4096 Items 或 1 MiB 时不截断并拒绝；Channel 有界；第 257 个 Worker 被拒绝；Append Outage 直接把已保留 Permit 从 Renewal 移入 Recovery，不暴露重新获取窗口。 | AC-6、AC-9、AC-15、AC-16 | Pass — Durability/Context Cap、Bounded-query Inspection、原子 Reservation-transfer 与 Handoff Regression。 |
| Framework or trust-boundary rejection | 适用 — Invalid Identity/UTF-8/JSON/Media Type/Body/Method。 | HTTP/Axum Boundary | AC-13、AC-15 | Invalid Identity 在 Service 前返回 401；Malformed Input 返回自有 4xx；合法 JSON Parameter 被接受。 | AC-13、AC-15 | Pass — Identity、Runtime Transport、Media-type Test。 |

## 验收检查 [Required]

| 检查 ID | 子任务 | 二元验收点 | 前置条件或输入 | 验证方法 | 精确预期结果 | 预期证据 | 状态 | 实际结果与证据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Domain 与 Application Source 不依赖 Axum、Provider Wire 或 Persistence Adapter。 | T-1 Source 已存在。 | 运行 `cargo test -p koduck-ai --test architecture domain_and_application_dependencies_are_inward -- --exact`。 | Exit Code 0；测试报告 `src/domain/**` 与 `src/application/**` 中 Forbidden Import 或 Cargo Dependency 数量均为 0。 | Command Output 和 Tested Commit。 | Pass | `46f2a39` 上 Exit 0，Forbidden Import 为 0。 |
| AC-2 | T-1 | 相同已认证输入与确定性 Provider Stream 产生一个与 Adapter Representation 无关的有序 Turn Lifecycle。 | 进程内 Provider 发出 Delta `A`、`B`、Usage 与 Completion；In-memory History Port 接受全部 Append。 | 运行 `cargo test -p koduck-ai --test cand_1_kernel tool_free_turn_completes_with_ordered_items -- --exact`。 | Exit Code 0；一个 Turn 进入 `completed`；Replay 顺序为 Input、`A`、`B`、Usage、Terminal；每个 Published Sequence Number 等于其 Durable Append Sequence。 | Command Output 和 Serialized Replay Fixture。 | Pass | `46f2a39` 上 Exit 0；Replay 与 Durable Publish Sequence 完全符合预期。 |
| AC-3 | T-1 | Provider Terminal Error 产生 `failed`，且绝不产生 `completed`。 | 进程内 Provider 发出 `A`，随后发出 Error Code `UPSTREAM_RESET`。 | 运行 `cargo test -p koduck-ai --test cand_1_kernel provider_error_is_failed_terminal -- --exact`。 | Exit Code 0；Durable Replay 包含 Input、`A` 和恰好一个携带 `UPSTREAM_RESET` 的 `failed` Terminal；`completed` Terminal 数量为 0。 | Command Output 和 Replay Fixture。 | Pass | `46f2a39` 上 Exit 0；只有一个 `UPSTREAM_RESET` Failed Terminal。 |
| AC-4 | T-2 | 同步 Route 符合自有 REST v1 契约。 | 有效 Trust Context；Input `hello` 的新 Thread Request；确定性 Provider Response `A`；按本 ADR 契约生成 Golden Fixture。 | 运行 `cargo test -p koduck-ai --test cand_1_contract sync_chat_v1_contract -- --exact`。 | Exit Code 0；Status `200`；`Content-Type` 为 `application/json`；Canonicalized Body 恰好包含 `thread_id`、`turn_id`、`status`、`items`、`usage`；`status` 为 `completed`；ID 均为 UUID；Item Sequence 为严格递增正整数；仅把 UUID 与 Usage Counter 替换为 Fixture Token 后，Body/Required Header 等于自有 v1 Golden Fixture。 | Command Output、Fixture Hash 和 Comparison Report。 | Pass | `46f2a39` 上 Exit 0；Normalized Response 与记录的 Fixture Hash 一致。Commit `d444cf3` 进一步证明超过 Axum Extractor Limit 的 Body 返回自有 `400 invalid-request` Problem，非 POST Request 返回自有 `405 method-not-allowed` Problem，二者均包含精确 Field 与 UUID Correlation ID。 |
| AC-5 | T-2 | SSE Route 符合自有 Event Contract，且 Durable Append 前不发布 Event。 | 有效 Trust Context；Provider 发出两个 Delta 和 Completion；记录自有 v1 Fixture Hash。 | 运行 `cargo test -p koduck-ai --test cand_1_contract sse_v1_contract_and_append_before_publish -- --exact`。 | Exit Code 0；Status `200`；Content Type 为 `text/event-stream`；Event 为一个 `turn.started`、两个有序 `item.created` 与恰好一个 `turn.completed`；全部 ID 一致且 Sequence 严格递增；每个 Publish Observation 都有同一 Item/Terminal Identity 且序号更低的成功 Append Observation。 | Command Output、Fixture Hash 和 Append/Publish Trace。 | Pass | `46f2a39` 上 Exit 0；SSE Fixture 与 Append-before-publish Trace 通过。Commit `a7258bc` 进一步证明 SSE Terminal 发出后发生 Replay Failure 时，不会追加相互矛盾的 `event: error`。 |
| AC-6 | T-2 | Resume 在同一 Thread 创建新 Turn，且不修改或静默截断此前 Terminal History。 | 已存在一个 Completed Turn 及其不可变 Replay Hash；越界 Fixture 包含 4097 个 Prior Items 或超过 1 MiB 规范序列化 Payload。 | 运行 `cargo test -p koduck-ai --test cand_1_contract` 与 PostgreSQL History Unit Test。 | Exit Code 0；预算内 Resume 使用同一 Thread 的新 Turn ID，并恰好一次提供有序 Durable History；越界 History 不创建新 Turn/Provider Call，返回自有 `400 invalid-request`；Prior Replay 不变。 | Command Output、前后 Replay Hash、Bounded-query Inspection 与 Response Assertion。 | Pass | Local Contract/PostgreSQL History Regression 通过；生产 Query 最多流式读取 4097 个有序 Row，拒绝第 4097 个 Item 或第 1,048,577 个序列化 Byte，且不构造越界 Provider Request。 |
| AC-7 | T-2 | Interrupt Contract 精确，且已认证 Client Stop 与 Platform/Dependency Cancellation 可区分。 | 一个 Owner Live SSE Turn、一个 Recovery-pending Turn、一个 Expired Started Turn、一个未知 UUID、一个非 Owner Turn、一个已终态 Turn，以及两个 Provider-terminal Competitor。 | 运行 Owned Contract 与生产 PostgreSQL Contract Test。 | Exit Code 0；仅在 Interrupt Transaction 已为未 Fenced/未过期的 `started` Owner Append 恰好一个 Durable `interrupted` Terminal 后返回 202；两个 Provider-terminal Competitor 均收到 `AlreadyTerminal`；Recovery-pending/Expired Started 拒绝 Interrupt；Unknown/Non-owned 仍不可区分；Dependency Cancellation 保持独立。 | Command Output、Normalized Response Hash、PostgreSQL Replay 和 Race Result。 | Pass | Local Contract/Arbitration Test 通过；生产 PostgreSQL 证明 Interrupt Terminal 在 Completion/Failure 并发者均失败前已经 Durable，并拒绝 Recovery-pending/Expired-started Fixture；Provider Request Establishment 仍可 Interrupt。 |
| AC-8 | T-3 | 初始 History Failure 不暴露 Accepted Turn；后续 Durability Outage 只暴露 Durable Prefix 与 `durability-unavailable`，并保留 Failed-recovery Ownership。 | Fault Adapter 在 Case A 让初始 Acceptance 失败；在 Case B 的一个 Durable Delta 后让下一次 Append 失败；在 Case C 让 Accepted Control-state Read 失败。 | 运行 `cargo test -p koduck-ai --test cand_1_durability`。 | Exit Code 0；Case A 的 Accepted Turn Record 和 Provider Call 均为 0；Case B/C 不发布未提交 Payload、停止 Provider Consumption、发出 `durability-unavailable`（在 SSE Route 上为恰好一个携带该 Problem Body 的带内 `error` Event，且无 Terminal Event）、进入 Recovery-pending，并通过 Liveness Handoff 调度 Failed Recovery。 | Command Output、Adapter Trace 和 Replay Fixture。 | Pass | Local Durability Regression 证明初始失败零副作用、Append Failure 只暴露 Durable Prefix、Accepted Control-read Failure Fail Closed，且两种 Accepted Outage 均保留 Liveness Recovery Reservation。 |
| AC-9 | T-3 | Append、Commit-reconciliation 与 Unpublished Buffer Limit 精确且 Fail Closed。 | Virtual Clock；分别测试 2.001 秒 Append、卡死的稳定 Identity Reconciliation、65 Items、Payload Size 1,048,577 Bytes。 | 运行 `cargo test -p koduck-ai --test cand_1_durability append_deadline_and_buffer_caps -- --exact` 与 `cargo test -p koduck-ai adapters::history::postgres::tests`。 | Exit Code 0；每个 Database Attempt 在 2 秒停止；Operation 与 Reconciliation 最多等待 4 秒；对账仍不确定时返回 `Unavailable`；每个 Buffer-limit Case 都停止 Provider Consumption、发布 0 个超限 Item、发出 `durability-unavailable`，且保留 Case 前 Durable Prefix。 | Command Output 和每个 Case 的 Trace。 | Pass | Local Regression 证明 Operation 与独立 Reconciliation 均受 Database Deadline 限制，包括卡死的 Reconciliation；现有 2.001 秒、Item 65 与 Byte 1,048,577 Case 均停止消费、映射到 `durability-unavailable` 且不发布超限 Item。Commit `a7b6faa` 证明 JSON Escape 与 Payload Object Overhead 均计入 Serialized 1-MiB Boundary。 |
| AC-10 | T-3 | Process Crash 对账 Fence 过期 Generation，并 Append 一个孤儿 `cancelled` Terminal。 | Virtual Clock；最后续租 t=0；Heartbeat 5 秒、Lease 20 秒、Clock-skew Margin 2 秒；Owner Process 在一个 Durable Delta 后立即终止。 | 运行 `cargo test -p koduck-ai --test cand_1_liveness process_crash_fences_and_cancels_once -- --exact`。 | Exit Code 0；t=22 秒前对账被拒绝；t=22 秒时 Fence Generation；Delta 后恰有一个 Durable `cancelled` Terminal；Fencing 后每个 Old-generation Append 返回 `FENCED`。 | Command Output 和 Lease/Append Trace。 | Pass | `46f2a39` 上 Exit 0；21.999 秒过早，22.000 秒 Cancelled 一次并 Fence。Commit `a7b6faa` 进一步证明 Request Shutdown 不会同步等待卡死的 Renewal Worker。 |
| AC-11 | T-3 | 并发 Reconciler 与延迟 Store Recovery 不能重复或覆盖 Orphan Terminal。 | 32 个 Reconciler 在 Store 不可用时争抢同一过期 Thread/Turn/Generation，随后 Store 恢复。 | 运行 `cargo test -p koduck-ai --test cand_1_liveness concurrent_reconcilers_are_idempotent -- --exact`。 | Exit Code 0；恢复后恰有一个 Conditional Write 成功；Durable History 恰有一个 `cancelled` Terminal；31 个 Reconciler 收到 `ALREADY_TERMINAL` 或 `FENCED`；Late `completed` Append 被拒绝。 | Command Output、Race Summary 和 Replay Hash。 | Pass | `46f2a39` 上 Exit 0；恢复后 1 个成功、31 个 Terminal/Fenced，Late Completion 被拒绝。 |
| AC-12 | T-3 | CAND-1 不得运行时依赖或 Fallback 到前身基础设施、Memory 或 Multitask。 | T-1 至 T-3 Source、Manifest、Configuration Schema 和 Migration 已存在。 | 运行 `cargo test -p koduck-ai --test architecture cand_1_has_no_legacy_or_external_history_fallback -- --exact`。 | Exit Code 0；Dependency Inspection 报告前身 Repository/Artifact/Route Identifier 为 0，CAND-1 Execution Graph 中 Memory/Multitask Client 为 0，且只配置一个权威 `TurnHistory` 实现：AI 自有 PostgreSQL Adapter。 | Command Output 和 Dependency/Configuration Report。 | Pass | `08cc1b3` 上 Exit 0；Concrete SQLx History、Reqwest Provider、Axum Runtime/Configuration、Executable Entry Point、Manifest 与幂等 Migration 满足前置条件；Inspection 找到 0 个禁止 Fallback Identifier，且生产 `TurnHistory` 仅有 `PostgresTurnHistory`。 |
| AC-13 | T-2 | 无 Validated Trust Context 的 Request 不得抵达 Application Turn Runner 或 Provider/History Port。 | Request 缺失或携带无效 Identity；加载自有 v1 Error Contract。 | 运行 `cargo test -p koduck-ai --test cand_1_contract invalid_identity_stops_at_presentation_boundary -- --exact`。 | Exit Code 0；Status `401`；`WWW-Authenticate` 为 `Bearer`；Content Type 为 `application/problem+json`；Body 恰好包含 `type: about:blank`、`title: Invalid identity`、数值 `status: 401`、`code: invalid-identity` 与 UUID `correlation_id`；Provider Call、Initial History Write 和 Accepted Turn 数量均为 0。 | Command Output、Response Fixture Hash 和 Adapter Call Counter。 | Pass | `46f2a39` 上 Exit 0；Fixture Hash 一致且 Service Call 为 0。 |
| AC-14 | T-1 | 根 Scope Routing 明确治理新的维护型 `koduck-ai/**` Source 与 Configuration Path。 | 根 `AGENTS.md` Scope Routing Table 与新 Workspace Manifest 存在。 | 确定性检查 Scope Routing Table 中恰好一个 `koduck-ai/**` Row。 | 恰好一个 Row 指定 `koduck-ai/**`，要求读取 `docs/README.md`、公共软件工程标准与 Rust 标准，以仓库根为 Working Directory，并列出非交互 Format、Lint、Test Command；该 Row 说明受治理 Build Command 仍需要 Accepted OCR。 | Scope Routing Row、Structured Inspection Result 和 Tested Commit。 | Pass | `46f2a39` 上 Structured Inspection 找到恰好一个完整 Scope Routing Row。 |
| AC-15 | T-2 | HTTP Media Type、同步 Terminal 与 Context-limit Mapping 精确。 | 带 `Application/JSON; charset=utf-8` 的有效 JSON，以及 Completed/Interrupted/Cancelled/Failed/Over-budget Resume Result。 | 运行 `cargo test -p koduck-ai --test cand_1_contract`。 | Exit 0；两条 Chat Route 接受 Parameterized JSON；Completed 返回 200，Interrupted/Cancelled 返回各自 409 Code，Failed 返回 `503 provider-unavailable`，Over-budget Resume 返回 `400 invalid-request`。 | Command Output 与 Response Assertion。 | Pass | Local Review Correction 的 10 项 Contract Test 全部通过，包括自有 Context-limit Problem Mapping。 |
| AC-16 | T-3 | Database Call 与后台 Liveness/Recovery Work 有界且 Fail Closed。 | Slow Database Future、Limit=1 的 Background Admission、Liveness 持有容量时的 Append Outage，以及 Recovery-thread Creation Failure。 | 运行 PostgreSQL Adapter/Recovery Test、Startup-timeout Test、`append_outage_confirms_admission_handoff_before_recovery` 与 Production Bound Architecture Test。 | Exit 0；慢调用返回 Typed Timeout/Unavailable；第 257 个 Worker 被拒绝；普通 Guard Drop 非阻塞；同一个 Reserved Permit 从 Renewal 移入 Recovery 且无 Acquisition Window；原 Observer 保持附着直至有界 Recovery 完成或交由对账；Recovery-thread Spawn Failure 在保留 Permit 时同步执行有界 Recovery。 | Command Output 与 Source Inspection。 | Pass | Deadline、Shared Admission、原子 Transfer、Terminal Replay 与注入 Spawn-failure Regression 通过；生产 Renewal/Recovery 仍共用 256 上限。 |
| AC-17 | T-1 | Provider Operation 不得超过自有 Deadline 持续 Pending。 | Production Reqwest Assembly 与 Provider Response Pump 已存在。 | 运行 Provider Unit Test 与 `architecture::production_io_and_background_work_are_bounded`。 | Exit 0；Connect Timeout 5 秒，Header/Idle/Total Timeout 为 30/30/120 秒，且返回 Stable Error Code。 | Command Output 与 Source Inspection。 | Pass | Local TCP Behavior Test 和 Production Deadline Regression 通过。 |

允许的最终检查状态为 `Pass`、`Fail` 或 `N/A — <具体原因>`。`Fail` 会阻止完成。
只有可证明检查触发条件或前置条件不适用时，`N/A` 才有效。

## 完成检查表 [Required]

| ID | 项目 | 完成条件 | 预期证据 | 状态 | 实际证据 |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR 已审批 | 记录合格非作者审批人、审批时间和精确 `Approval Evidence: Approve`；可选 Approval Context Revision 仅为信息性、非约束，且准确表示获批内容 | ADR Metadata | Complete | `@linhai` 明确 ADR-0001 与 PR #2 Review Thread `3764738232` 并提供精确 `Approve`；元数据记录 `2026-08-12T16:25:02+08:00`。Implementation Commit `8de786d` 是稳定交付证据；本次审批有意绑定 Task Context 而非 Revision，因此不记录 Approval Context Revision。 |
| A-2 | 完整任务已交付 | 每个已声明子任务都有实际实施证据；每个适用验收检查均为 `Pass` 且有实际结果和证据；它们共同满足完整任务结果 | Implementation Plan 与 Acceptance Checks Row | Complete | T-1 至 T-3 均为 `Complete`；AC-1 至 AC-17 均为 `Pass`；当前 Review Correction Worktree 通过 Routed Format、严格 Clippy 与完整 Test Gate。 |
| A-3 | 适用时同步 ADD 双向链接 | Selected Candidate 记录本 ADR 精确路径，本 ADR 记录精确 ADD 路径和 Candidate ID，双方一致；只有本 ADR 为 `Complete`/`Verified` 后 Candidate 才到 `Complete` | ADD Path、Candidate ID、ADR Path 和 Git Blob/Commit | Complete | 本完成变更保持 `Architecture Source` 为 `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-1，并原子地把该 Candidate 更新为 `Complete`，记录本 ADR 路径和 `Accepted`、`Complete` Evidence。 |
| A-4 | 满足要求级别 | 每个 Required Section 完整；每个 Conditional Trigger 已评估并完成或标为 `N/A — <原因>`；Optional Section 完整或删除 | Structured Document Review | Complete | 结构化评审确认新增 Scope Routing 交付物及当前阶段其他 Required/Triggered 内容均完整；实施阶段证据仍由 A-2 与验收检查行治理。 |
| A-5 | 验收检查可判定 | 每个检查指定一个 Subtask、Precondition/Input、Deterministic Method、Exact Expected Result 和 Evidence，且无无约束主观标准 | Structured Acceptance-check Review | Complete | 结构化检查确认恰好 17 项检查；每项均包含一个 Subtask、非空 Precondition、确定性 Method、精确可观察 Expected Result 与 Evidence Field。 |
| A-6 | 适用时治理工程例外 | 每个超出或豁免规则都有完整 Exception Row、Accountable Owner、Lifecycle 和 Verification Evidence；否则条件章节记录 `N/A — <原因>` | Engineering Exceptions 与 Affected-file Evidence | N/A — 未提出例外 | Engineering Exceptions 记录 `N/A`；实施发现例外时必须执行使审批失效的更新。 |

## 补充说明 [Optional]

- 前身基线 Commit `c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe` 展示了
  Chat/Stream 功能场景、`LlmProvider` 抽象和 OpenAI-compatible Adapter。它只
  是功能调研证据；其中任何 Route、Field、Artifact、Datastore 或 Fallback 行为
  都不是 CAND-1 权威要求。
- 2 秒 Append Deadline、64-Item/1-MiB Unpublished Cap、5 秒 Heartbeat、
  20 秒 Lease 和 2 秒 Clock-skew Margin 都是对审批敏感的决策值。Accepted
  后改变任一值，都必须执行使审批失效的流程。
- 当前 Worktree 的提交前分解评审：超过 400 行评审阈值的生产文件为
  `sqlx_executor.rs`（739）、`provider/mod.rs`（582）、`runner.rs`（591）、
  `postgres.rs`（509）和 `runtime/mod.rs`（423）；超过 600 行评审阈值的测试
  文件为 `turn_terminal_arbitration.rs`（870）、`runtime_wiring.rs`（762）和
  `cand_1_durability.rs`（679）。它们均低于 800/1,200 行例外上限。生产文件
  分别保持单一边界：事务型 PostgreSQL State、OpenAI-compatible Streaming
  Protocol、Turn Orchestration、PostgreSQL Liveness/History Adaptation 和 Runtime
  Assembly；测试文件各自保持单一 Contract Family。超过 60 行的可执行单元为
  Turn Execution Coordinator、四个事务型 Acceptance/Append/Recovery/
  Reconciliation Method，以及两个端到端 Arbitration Scenario；每个都保持一个
  对顺序敏感的 Operation 或 Fixture 在同一视野内，且均未超过 120 行例外上限。
  进一步抽取透传单元会割裂 Transaction、Lifecycle 或 Scenario Invariant，不能
  降低耦合。Cyclomatic Complexity 为 `N/A — 未配置复杂度工具`；Source
  Inspection 未发现达到例外上限的嵌套。本项不需要且未声明 Engineering
  Exception。

## 归档 [Conditionally Required — Decision Status 为 `Rejected`，或 Decision Status 为 `Deprecated`/`Superseded` 且 Implementation Status 为终态]

当 Decision Status 为 `Rejected` 且 Implementation Status 为 `Not Applicable`，
或 Decision Status 为 `Deprecated`/`Superseded` 且 Implementation Status 为
`Verified`、`Complete` 或 `Not Applicable` 时，应在同一变更中归档本记录。
触发前保留本节作为未激活的未来生命周期说明；其检查表不影响审批或实施完成。
触发时：

- [ ] 将英文权威文件移动到本项目 ADR Root 下的
      `archive/ADR-0001-provider-neutral-turn-kernel.md`；本翻译保留在当前路径，
      并把英文权威链接更新为 `../../archive/ADR-0001-provider-neutral-turn-kernel.md`，
      同时更新归档后英文文件指向本翻译的相对链接。
- [ ] 把引用归档前英文路径的所有 Code Marker 更新为新 Archive Path；若受治理
      Code 已删除，则移除 Marker。
- [ ] 若 Decision Status 为 `Superseded`，把 Replacement Record 的 `Supersedes`
      与本记录的 `Superseded By` 设置为彼此最终仓库相对路径。
- [ ] 若无记录取代本记录，保留 `Superseded By: None`。
- [ ] 更新 `docs/adr/INDEX.md` 中本记录唯一行的归档路径、范围和最终状态；不得为
      本中文翻译新增独立行。
- [ ] 确认 Archive 外没有 ADR/OCR 或 Code Marker 继续引用归档前英文路径。

## 变更日志 [Required]

| 日期 | 变更 | 作者 |
| --- | --- | --- |
| 2026-08-11 | 通过选择 CAND-1 创建 Proposed 项目级 Full ADR，包含详细边界、精确时序与 Buffer 决策、三个子任务、确定性验收检查和未解决的审批前置条件。 | @codex |
| 2026-08-11 | 创建非权威中文翻译，并从英文权威 ADR 建立链接；未创建第二个决策身份或索引行。 | @codex |
| 2026-08-11 | 将完成检查表 A-3 至 A-5 从 `In Progress` 恢复为 `Not Started`，并使实际证据与尚未解决的审批前置条件一致。 | @codex |
| 2026-08-11 | 按当前 Codex 任务中的仓库 Owner 指示解决 Q-1 至 Q-3：以新 REST/SSE v1 与 AI 自有 PostgreSQL History Contract 为权威，移除旧 Parity、共享 History、APISIX Route-back 与运行时 Fallback 要求，并按 Greenfield 边界重写子任务与验收检查。 | @codex |
| 2026-08-11 | `@linhai` 于 `2026-08-11T10:37:34+08:00` 把 ADD-0001 重新批准为 `Current` 后，同步架构来源 Gate；ADR Decision Status 仍独立保持 `Proposed`。 | @codex |
| 2026-08-11 | 人类审批人先自声明 `@linhai`、明确 ADR-0001，再精确回复 `Approve`，从而接受本 ADR；记录 Approval Time `2026-08-11T10:40:42+08:00`。Implementation Status 保持 `Not Started`；由于获批内容尚无对应不可变 Commit，不记录 Approval Context Revision。 | @linhai |
| 2026-08-11 | `2026-08-11T10:53:55+08:00` 的使审批失效修订，在仓库首次增加维护型 `koduck-ai/**` Source/Configuration Path 前加入根 `AGENTS.md` Scope Routing 交付物和 AC-14。保留旧审批：Approver `@linhai`、Approval Time `2026-08-11T10:40:42+08:00`、Approval Evidence `Approve`、无 Approval Context Revision。Decision Status 重置为 `Proposed`，Implementation Status 保持 `Not Started`，等待重新审批。 | @codex |
| 2026-08-11 | 人类审批人自声明 `@linhai`、明确 ADR-0001 并提供精确 `Approve`，从而重新批准 Scope Routing 修订；记录 Approval Time `2026-08-11T11:02:27+08:00`，Decision Status 恢复为 `Accepted`。Implementation Status 保持 `Not Started`；由于获批内容尚无对应不可变 Commit，不记录 Approval Context Revision。 | @linhai |
| 2026-08-11 | 接受后启动 T-1：加入必需的 `koduck-ai/**` Scope Routing 行、Cargo Workspace Manifest 和 Test-first Domain Lifecycle Specification。Production Source 有意保持缺失，等待受治理的红灯测试构建。 | @codex |
| 2026-08-11 | `2026-08-11T11:07:52+08:00` 的使审批失效修订，在任何受治理 Build 或 Production Source 创建前补入遗漏的 `Cargo.lock`、`koduck-ai/src/lib.rs` 与 `koduck-ai/src/adapters/mod.rs`。保留旧审批：Approver `@linhai`、Approval Time `2026-08-11T11:02:27+08:00`、Approval Evidence `Approve`、无 Approval Context Revision；Decision Status 重置为 `Proposed`，Implementation Status 重置为 `Not Started`。 | @codex |
| 2026-08-11 | 人类审批人自声明 `@linhai`、明确 ADR-0001 并提供精确 `Approve`，从而重新批准完整维护路径范围；记录 Approval Time `2026-08-11T11:14:45+08:00`，Decision Status 恢复为 `Accepted`，Implementation Status 为 T-1 进入 `In Progress`。尚不记录 Approval Context Revision。 | @linhai |
| 2026-08-11 | 记录 Implementation Commit `af10ac9`、`4a7bf5d`、`46f2a39` 与 `80fc2ff`；T-1/T-2 Complete，AC-1 至 AC-11 及 AC-13/AC-14 Pass。T-3 保持 In Progress，因为满足 AC-12 前置条件所需的 Concrete PostgreSQL Executor、HTTP Runtime/Configuration 与 Executable Entry Point 尚未实现。 | @codex |
| 2026-08-11 | 记录 Dependency Lock Operation Commit `1e00208` 与 Runtime Implementation Commit `08cc1b3`；完成 T-3 与 AC-12，确认全部 14 个验收检查、Format、严格 Clippy 与 20 个测试通过，将 Implementation Status 设为 `Complete`，并同步 ADD-0001 CAND-1 为 `Complete`。 | @codex |
| 2026-08-11 | 记录 Review Correction Commit `56073a0`：移除 Request-wide Serialization，使 SSE 与 OpenAI-compatible Frame Consumption 增量化，将 Accept 后 Provider Setup Failure 终态化，对生产 Append 强制两秒 Deadline，并接线 Lease Renewal/Orphan Reconciliation Worker。Format、严格 All-target Clippy 与全部 25 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-11 | 记录第二个 Review Correction Commit `df49b69`：以内联方式交付 Post-start SSE Failure，在 Provider Idle 时轮询 Interrupt，以结构化方式解码 Nullable Usage，把同步 Failed Turn 映射为 `503`，执行 Runtime Payload Cap，保持认证优先级地拒绝非法 UTF-8，并重试临时 Lease-renewal Failure。Format、严格 All-target Clippy 与全部 32 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-11 | 记录第三个 Review Correction Commit `11b5ea2`：在有界所有权下把 Durability Recovery 从 `recovery-pending` 关闭为 `failed` 或交由 Fencing Reconciliation，强制 Resume/Interrupt 的 Subject Isolation，按 Turn 合并历史流式 Delta，接受标准 JSON Escape 且继续拒绝 Duplicate/Unknown Field，并序列化完整控制字符范围。Format、严格 All-target Clippy 与全部 38 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-11 | 记录第四个 Review Correction Commit `fe3beb9`：拒绝非 HTTPS Provider Endpoint，为 Runtime Failure 加入 UUID Correlation ID，以 PostgreSQL Transactional Arbitration 让已接受 Interrupt 优先于 Completion，并在 Live Turn Execution 中强制 64-Item Limit。Format、严格 All-target Clippy 与全部 42 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-11 | 记录第五个 Review Correction Commit `a7258bc`：让已接受 Interrupt 优先于每一种 Provider Terminal，把 PostgreSQL 仲裁保持在单次有界 Append Operation 内，并在 SSE Terminal 已发出后抑制相互矛盾的 Error。Format、严格 All-target Clippy 与全部 45 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-11 | 记录第六个 Review Correction Commit `a7b6faa`：计算包含 JSON Escape 的规范 Serialized Payload Bytes，在 Consumer Stream 被丢弃时取消 Provider Response Pump，并使 Renewal Guard 在数据库调用卡死时非阻塞退出。Format、严格 All-target Clippy 与全部 47 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-11 | 记录第七个 Review Correction Commit `31ef43f`：在 Response Header Pending 时返回有界 Provider Poll，在 Consumer 关闭时取消 Request Establishment，并在 Pending Buffer 超过限制前拒绝大于 1 MiB 的未终止 Provider Frame。Format、严格 All-target Clippy 与全部 49 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-11 | 记录第八个 Review Correction Commit `d444cf3`：使并发同 Thread History 按 Turn 保持连续，把已认证超大 Body 映射为自有 `400 invalid-request` Problem，并把不支持的 Method 路由到自有 `405 method-not-allowed` Problem。Format、严格 All-target Clippy 与全部 52 个测试通过；本次仅更新证据，不改变已接受的 Decision 或 Scope。 | @codex |
| 2026-08-12 | 仓库 Owner `@linhai` 在当前 Codex 任务中明确回复 `确认Approve`，授权把当前 7 项 Review Correction 作为 ADR-0001 已批准范围内的缺陷修复，不重开已完成的 CAND-1，也不因 ADR-0002 序列化而新建 ADR。补齐必需的 Contract Traceability 与五行 Risk Matrix，修正 HTTP Terminal/Media-type 行为，为所有 Database/Provider Wait 和后台 Renewal/Recovery Admission 加入边界，并更新过时仓库说明。依据该 Owner Determination，Decision Status 保持 `Accepted`，Implementation Status 保持 `Complete`。 | @linhai |
| 2026-08-12 | Review `4912786010` 后续纠错：Append Outage 在调度 Recovery 前释放该 Turn 的 Renewal Permit，使 256 个续租任务满载时仍能把容量移交给 Recovery；PostgreSQL Connection 与 Migration Startup 分别使用 2 秒 Deadline。补充确定性 Handoff/Startup-timeout Regression；本次不改变已接受 Scope 或状态。 | @codex |
| 2026-08-12 | Review `4912891981` 后续纠错：用显式 Recovery Handoff 替代异步 Renewal-stop Signal；该 Handoff Join 受 Deadline 约束的 Renewal Worker，并确认 Permit 已释放后才调度 Recovery。普通 Request-shutdown Drop 保持非阻塞。补充 Permit 可用性与普通 Drop 的回归测试；本次不改变已接受 Scope 或状态。 | @codex |
| 2026-08-12 | Review `4912970984` 后续纠错：通过单 Owner Channel 把 Renewal Worker 的现有 Permit 直接移动到 Recovery，消除最后的 Release/Reacquire Window。Reservation 在交接全程保持占用，因此并发 Turn 无法在满载时抢走 Slot。补充原子 Reservation Regression，并保留普通非阻塞 Drop Regression；本次不改变已接受 Scope 或状态。 | @codex |
| 2026-08-12 | 提交前整体 PR Review 修正三个相邻 Lifecycle Gap：Accepted Control-state Read Outage 进入 Failed Recovery，`recovery-pending` 接受已认证 Interrupt，`AlreadyTerminal` Append Race 与 Fencing 一样回放 Durable Winner。另将 Cargo Package Metadata 与仓库 MIT License 对齐，以行为测试替换 Implementation-text Assertion，并刷新分解证据；本次不改变已接受 Scope 或状态。 | @codex |
| 2026-08-12 | Review `4913319728` 修正 Recovery/Stream Boundary：仅未 Fenced、未过期的 `started` Owner 可接受 Interrupt；已脱离 Stream 的 Recovery-pending 与 Expired Owner 拒绝。原子 Liveness Handoff 现在在释放原 Observer 前完成有界 Recovery 并回放 Durable Terminal；Recovery-worker 创建失败时保留 Permit 并执行有界同步 Fallback。补充生产 PostgreSQL Rejection 与注入 Spawn-failure Regression；本次不改变已接受 Scope 或状态。 | @codex |
| 2026-08-12 | Review `4913411000` 通过让 Interrupt Endpoint 自身成为 Terminal Transaction，消除最后的 Near-expiry Acceptance Race：锁定 Owned Live Turn/Lease，插入唯一 `interrupted` Item，推进 Status/Sequence，并在返回 202 前提交。即使随后立即过期，并发 Provider/Recovery Writer 也只会回放已 Durable 的 Winner。更新生产 PostgreSQL Race Regression；本次不改变已接受 Scope 或状态。 | @codex |
| 2026-08-12 | `2026-08-12T15:55:49+08:00` 的使审批失效 Review `4913815805` 修订增加公开的 Resume 越界 `400 invalid-request` 结果，并改变 Commit/Recovery 行为。保留旧审批：Approver `@linhai`、Approval Time `2026-08-11T11:14:45+08:00`、Approval Evidence `Approve`、无 Approval Context Revision；Decision Status 重置为 `Proposed`，Implementation Status 重置为 `Not Started`，等待重新审批。 | @codex |
| 2026-08-12 | 仓库 Owner `@linhai` 明确 ADR-0001 并精确回复 `Approve`，重新批准 Review `4913815805` 的三项修订；记录 Approval Time `2026-08-12T15:55:49+08:00`，Decision Status 恢复 `Accepted`，修订后检查通过后 Implementation Status 恢复 `Complete`。获批修订按稳定 Item Identity 对账 Commit Acknowledgement 超时，把每次 Recovery Attempt 限于总计 22 秒 Window 的剩余时长，并把 Resume Context 限制为 4096 Items/1 MiB 且不截断。由于尚无不可变 Revision 表示本次获批修订，因此不记录 Approval Context Revision。 | @linhai |
| 2026-08-12 | 记录 Review `4913815805` 的 Implementation Commit `1099bfe`。路由的 `cargo fmt --all --check`、严格 All-target/All-feature Clippy 与完整 All-target/All-feature Suite 全部通过，共 101 项测试，包含 PostgreSQL Writer/Reconciliation Lock 串行化、失败/超时 Commit Acknowledgement 对账、精确剩余 Window Recovery Attempt、两种 Aggregate Resume Budget 与自有 HTTP Mapping。本次纯证据更新不改变已批准 Decision、Behavior 或 Scope。 | @codex |
| 2026-08-12 | `2026-08-12T16:25:02+08:00` 的使审批失效 PR #2 Review Thread `3764738232` 修订为此前无界的稳定 Identity Reconciliation Attempt 增加边界。保留旧审批：Approver `@linhai`、Approval Time `2026-08-12T15:55:49+08:00`、Approval Evidence `Approve`、无 Approval Context Revision；Decision Status 重置为 `Proposed`，Implementation Status 重置为 `Not Started`，等待重新审批。 | @codex |
| 2026-08-12 | 仓库 Owner `@linhai` 明确 ADR/Review 冲突并精确回复 `Approve`，重新批准 Bounded Commit-reconciliation 修订；记录 Approval Time `2026-08-12T16:25:02+08:00`，Decision Status 恢复 `Accepted`，修订后检查通过后 Implementation Status 恢复 `Complete`。获批行为把 Write 与稳定 Identity Reconciliation 限制为两个各自独立的 2 秒 PostgreSQL Attempt；若对账仍不确定则返回 `Unavailable`。由于尚无不可变 Revision 表示本次获批修订，因此不记录 Approval Context Revision。 | @linhai |
| 2026-08-12 | 记录 PR #2 Review Thread `3764738232` 的 Implementation Commit `8de786d`。路由 Format、严格 All-target/All-feature Clippy 与完整 All-target/All-feature Suite 全部通过，共 102 项测试，包含先红后绿的 Stalled-reconciliation Deadline Regression。本次纯证据更新不改变已批准 Decision、Behavior 或 Scope。 | @codex |
| 2026-08-17 | `2026-08-17T08:25:37Z` 的使审批失效契约调和：SSE Wire Contract 现在枚举实现本已针对 Stream 开始后失败发出的带内 `error` Transport Diagnostic Event，消除 Event Name 枚举与耐久性条款要求的 `durability-unavailable` 诊断之间的矛盾；CT-8 与 AC-8 现在约束该 Event 的精确 Problem Body 形状、不携带 `thread_id`/`turn_id`/`sequence`、无 Terminal Event 关闭 Stream，以及 Terminal Event 发布后禁止再发 `error` Event（`runtime_wiring::mid_turn_failure_is_reported_inside_an_started_sse_stream`；`sse_terminal_consistency::replay_failure_after_sse_terminal_does_not_emit_error_event`）。Source 行为未变。保留旧审批：Approver `@linhai`、Approval Time `2026-08-12T16:25:02+08:00`、Approval Evidence `Approve`、无 Approval Context Revision；Decision Status 重置为 `Proposed`，Implementation Status 重置为 `Not Started`，等待重新审批。 | @kimi |
| 2026-08-17 | 仓库 Owner 在活跃任务中自我声明 `@linhai`、明确 ADR-0001 并精确回复 `Approve`，重新批准 SSE `error` Event 的 Wire Contract 调和；记录 Approval Time `2026-08-17T08:57:39Z`，Decision Status 恢复 `Accepted`。由于尚无不可变 Revision 表示本次获批内容，因此不记录 Approval Context Revision。修订后验收检查重新执行通过后，Implementation Status 方可恢复。 | @linhai |
| 2026-08-17 | 重新审批后重新执行修订的验收检查：`cargo test -p koduck-ai --test cand_1_durability`（9 通过，AC-8/AC-9）、`cargo test -p koduck-ai --test runtime_wiring mid_turn_failure_is_reported_inside_an_started_sse_stream -- --exact`（1 通过）、`cargo test -p koduck-ai --test sse_terminal_consistency replay_failure_after_sse_terminal_does_not_emit_error_event -- --exact`（1 通过）、`cargo test -p koduck-ai --test cand_1_contract`（10 通过，覆盖 AC-4/AC-5/AC-6/AC-13/AC-15）；路由 `cargo fmt --all --check`、严格 All-target/All-feature Clippy 与完整 All-target/All-feature Suite 全部通过、0 失败。Implementation Status 恢复 `Complete`；本次纯证据更新不改变已批准 Decision、Behavior 或 Scope。 | @kimi |
