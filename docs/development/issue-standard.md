# Issue Standard

**Applies to**: GitHub issues opened in `hailingu/koduck` by humans or agents.
Issue bodies SHOULD use the language of the affected project documentation;
this standard is in English for agent interoperability.

**Last reviewed**: 2026-09-25

## Authority And Required Reading

Read [AGENTS.md](../../AGENTS.md), especially Work Coordination, Review Rounds
And Convergence, Change Classification, and Approval And Status. The
[Koduck Trello board](https://trello.com/b/Kz9qnd3D/koduck) remains the mutable
coordination view for demand, priority, ownership, and progress. GitHub issues
record actionable problems or proposals and their evidence; an issue title,
label, comment, or closure does not approve an ADD, accept an ADR/OCR, authorize
implementation, or replace a review-thread reply. Link an existing Trello card
or decision record when one governs the reported work; do not create one solely
to complete an issue form.

## Security Reports

Suspected vulnerabilities, exploit paths, credential leaks, and sensitive-data
exposure MUST NOT be reported through an ordinary public issue, comment, or
attachment. Report the technical details privately through GitHub's
[repository security advisories](https://github.com/hailingu/koduck/security/advisories)
using **Report a vulnerability** when that private-reporting action is
available. If it is unavailable, withhold technical details and obtain a
confidential reporting destination from the repository owner through an
existing private contact channel before sending the report. A public request
for a private contact route, if unavoidable, MUST contain no affected identity,
exploit steps, proof of concept, sensitive logs, or other vulnerability
details.

The owner coordinates private triage and containment. Only after the owner
approves a sanitized description MAY a public tracking issue be opened; it
MUST exclude exploit instructions and sensitive evidence. The title, severity,
body, and closing rules below apply to that approved tracking issue, not to the
private report.

## Title Format

Every issue title MUST have this shape:

```text
P<0|1|2|3>(<scope>): <summary>
```

- The `P` tag states the severity defined below. Do not add urgency words to
  the summary as a second severity claim.
- `<scope>` names one primary repository area using a short, lowercase token
  consistent with commit scopes where possible (for example `correction`,
  `provider`, `governance`, `sonar`, or `docs`). Use `repo` when the problem is
  genuinely cross-cutting. Split unrelated problems into separate issues.
- `<summary>` states the observed problem or proposed outcome in one line,
  without a trailing period. A proposed fix is not a substitute for the
  problem or outcome.
- Conventional Commit type words such as `fix` and `feat` classify commits;
  they MUST NOT replace the severity tag in an issue title.

Examples:

```text
P1(correction): 跨租户更正被错误纳入有效历史
P2(provider): 流结束后重复发出终态事件
P3(docs): 故障排查说明缺少数据库不可用场景
```

## Severity And Labels

Choose severity from the best supported trigger and impact, not from the
amount of work needed to repair the issue:

- **P0** — an active or readily repeatable critical incident, such as
  widespread data loss, confirmed sensitive-data exposure, or a core service
  outage requiring immediate containment.
- **P1** — realistically reachable data loss, corruption, or exposure, or a
  core workflow that cannot complete under ordinary use.
- **P2** — a contract or behavior defect with a narrow trigger, intermittent
  failure, limited impact, or a verified workaround, when its established
  impact does not meet P0 or P1.
- **P3** — defense in depth, documentation and maintainability gaps, or a
  nonurgent improvement without a demonstrated P0–P2 impact.

The body MUST explain the trigger, impact, and evidence supporting the chosen
severity. When more than one definition applies, choose the highest applicable
tier supported by evidence (P0 > P1 > P2 > P3); a narrow trigger,
intermittency, or workaround does not downgrade established P0/P1 impact. If
impact is uncertain, state any unverified higher-impact concern and reassess
promptly when triage establishes more facts. Do not present a speculative
impact as observed. A severity tag is triage metadata; the separate AGENTS.md
rules determine whether a review finding blocks a PR.
P0 or P1 classification does not override the private route for security
reports.

When severity labels are configured, a maintainer or automation with label
permission MUST apply exactly one `P0`, `P1`, `P2`, or `P3` label matching the
title during triage; a reporter MAY apply it when permitted. Filing remains
valid before that reconciliation. Any applied severity label MUST match the
title. An area label MAY match the scope. A maintainer or permitted reporter
SHOULD apply `known-boundary` when that label is configured and the issue
records a deliberate boundary or disposition rather than a confirmed defect.
Do not create or change repository labels as part of filing an issue without
the applicable governance authorization.

## Body Structure

One issue reports one independently triageable problem or outcome. Use the
matching headings below; mark unknown facts as unknown instead of inventing
reproduction, root cause, or test results. Security reports follow the private
route above; the headings below apply only to owner-approved sanitized public
tracking issues.

**Defect or regression** — keep these six headings in order:

```text
## 环境 (Environment)
## 复现步骤 (Steps to Reproduce)
## 期望结果 (Expected)
## 实际结果 (Actual)
## 证据 (Evidence)
## 回归来源 (Regression Source)
```

**Known boundary** — describe the case and the decision to retain it:

```text
## 边界描述 (Boundary)
## 触发条件 (Trigger)
## 现有防线 (Existing Defense)
## 处置 (Disposition)
```

**Feature or improvement** — describe the need without claiming approval:

```text
## 背景与目标 (Goal)
## 方案草案 (Draft Approach)
## 验收标准 (Acceptance Criteria)
```

For a defect, Environment identifies the platform, relevant configuration,
entry point, and known version. Distinguish the running version from the code
revision inspected. Steps to Reproduce gives numbered actions, the precise
trigger, whether the reporter reproduced it, and whether it is intermittent.
Expected states the observable behavior and guarantees to preserve; proposed
new behavior must be labeled as a proposal. Actual separates direct
observations from secondhand reports and explains the effect on the user.

Evidence SHOULD cite stable artifacts such as commit SHAs, symbols, accessible
logs or screenshots, and command output. Record the command's working
directory, result, and limits. Separate confirmed causes from hypotheses and
say what was not checked. Passing existing tests does not disprove a reported
defect. A constructed minimal example MUST be identified as constructed, not
as a captured request or log. Local temporary paths are not GitHub-accessible
attachments. Remove secrets, credentials, private endpoints, and unnecessary
sensitive data from all evidence. Focused repair ideas and observable
acceptance criteria MAY appear in Evidence, but do not authorize a contract
change. Regression Source names the introducing commit only when established;
otherwise write `Unknown — not yet identified`. An analysis baseline is not
automatically the introducing commit. Do not put vulnerability reproduction,
proof of concept, or uncontained exposure details in public Evidence.

For a known boundary, Disposition states who accepted or deferred the boundary,
the supporting evidence, any tracking link, and when to revisit it. For a
feature or improvement, Acceptance Criteria states observable outcomes; link
the relevant Trello demand and ADD/ADR/OCR when they exist. The issue remains
a proposal until the repository's governing process authorizes the work.

### Reusable Defect Template

Replace placeholders with supported facts. Keep the required headings and
remove optional details that do not apply.

````markdown
## 环境 (Environment)

- 平台、配置与入口：<已知事实；未知项注明待确认>。
- 运行版本：<版本或待确认>；分析代码版本：<如已检查，填写 commit SHA>。
- 严重性：P<0|1|2|3>。<触发条件、影响及已验证的绕过方式>。

## 复现步骤 (Steps to Reproduce)

1. <准备状态与入口>
2. <触发操作或输入>
3. <观察结果的位置>

复现状态：<用户报告 / 本地已复现；频率和未确认条件>。
<若使用最小示例，注明实际捕获或人为构造。>

## 期望结果 (Expected)

<可观察的正确结果，以及必须保留的既有保证。>

## 实际结果 (Actual)

<直接观察或用户报告的失败现象、诊断信息及影响，分别注明来源。>

## 证据 (Evidence)

<可访问的截图、日志或录屏链接；若证据只在报告对话中，说明限制。>

- 固定版本代码证据：<commit SHA、符号和行为关系；未检查则注明>。
- 原因分析：<已确认原因、待验证假设及缺失证据>。
- 验证情况：<工作目录、命令、结果、限制和未运行项>。
- 建议修复与验收：<如有，描述可观察的成功条件；方案仍待批准>。

## 回归来源 (Regression Source)

<已确认的引入提交及依据；否则写 Unknown — not yet identified。>
````

## Triage And Closing

Triage checks scope, severity, duplicates, evidence, and links to any governing
Trello card or decision record. A duplicate points to the canonical issue and
records why the cases share one underlying defect. For a repaired defect, close
only after the resolving commit is reachable from the intended delivery branch
or release, or the resolving PR has merged there, and the observable outcome is
verified. A linked open PR alone is not a delivered resolution. An issue may
also close after a reasoned duplicate, invalid, declined, or known-boundary
disposition is recorded with supporting evidence. State remaining risk and the
tracking link when work is deferred. Closing an issue does not resolve a GitHub
review thread or waive an AGENTS.md approval, acceptance, verification, or CI
gate.
