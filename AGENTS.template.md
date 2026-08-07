# {{PROJECT_NAME}} Agent Guide

## Template Instructions

Use this file to create a project-level agent guide.

- Replace every required variable listed below.
- Retain an optional module only when its policy applies, then replace all of
  that module's variables and remove its boundary comments.
- Remove unused optional modules completely.
- Duplicate repeatable rows once per scope or source, then replace their row
  variables.
- Treat `<example-metavariables>` as examples, not template variables.
- Remove this section and the Variable Dictionary from the instantiated guide.
- Remove the provenance marker when the project has no provenance policy.
- Finish with no unresolved `{{...}}` variables or optional-module comments.

## Variable Dictionary

### Required Variables

| Variable | Meaning |
| --- | --- |
| `{{PROJECT_NAME}}` | Project name used in the title |
| `{{DOCUMENT_LANGUAGE}}` | Language expected for the guide and project documentation |
| `{{PROJECT_SUMMARY}}` | One- or two-sentence project description |
| `{{INSTRUCTION_PRECEDENCE}}` | Precedence rule for repository-level and nested instructions |
| `{{CHANGE_CLASSIFICATION_POLICY}}` | How agents classify read-only, editorial documentation, normative/governance documentation, source, config, and operational work |
| `{{APPROVAL_POLICY}}` | Actions requiring approval; eligible AI/human approvers; concrete `@<actor-id>` evidence; self-approval prohibition; and identified-human escalation for material uncertainty |

### Conditional Variables

These variables are required only when their optional module or marker remains.

| Variable | Module or use |
| --- | --- |
| `{{PROVENANCE_REFERENCE}}` | Provenance marker: approved decision or policy reference |
| `{{MODIFIED_DATE}}` | Provenance marker: modification date in the project's chosen format |
| `{{DECISION_RECORD_POLICY}}` | Decision Records: changes requiring a record and available record weights |
| `{{DECISION_RECORD_ROOT}}` | Decision Records: authoritative record location |
| `{{DECISION_APPROVAL_POLICY}}` | Decision Records: eligible AI/human authority, concrete `@<actor-id>` identity, immutable evidence, self-approval prohibition, identified-human uncertainty escalation, and reapproval rules |
| `{{DECISION_ACCEPTED_STATE}}` | Decision Records: exact state that permits implementation |
| `{{DECISION_STATUS_POLICY}}` | Decision Records: decision states and legal transitions |
| `{{IMPLEMENTATION_STATUS_POLICY}}` | Decision Records: implementation states and legal transitions |
| `{{DECISION_IDENTITY_POLICY}}` | Decision Records: numbering, path-qualified identity, and local/project storage |
| `{{DECISION_PROVENANCE_POLICY}}` | Decision Records: marker placement and unsupported-format handling |
| `{{DECISION_EVIDENCE_POLICY}}` | Decision Records: stable approval and completion evidence |
| `{{OPERATIONAL_CHANGE_RECORD_POLICY}}` | Decision Records: eligibility and escalation boundary for operational records |
| `{{OPERATIONAL_CHANGE_TEMPLATE_PATH}}` | Decision Records: template path for operational records |
| `{{OPERATIONAL_CHANGE_SAFETY_POLICY}}` | Decision Records: core runbook fields and risk-triggered operational extensions |
| `{{REFERENCE_FRESHNESS_POLICY}}` | Reference Freshness: review window, refresh trigger, and offline behavior |
| `{{PROTECTED_BRANCH_POLICY}}` | Version Control: protected-branch rules |
| `{{TASK_BRANCH_POLICY}}` | Version Control: preservation of authorized branches/worktrees, new-branch naming and source rules, and whether remote synchronization is authorized |
| `{{PULL_REQUEST_POLICY}}` | Version Control: target, evidence, and review rules |
| `{{COMMIT_POLICY}}` | Version Control: commit-message and commit-scope policy |
| `{{AGENT_CONFIG_PATH}}` | Collaboration Discovery: agent-definition location |
| `{{SKILL_DIRECTORY}}` | Collaboration Discovery: reusable-skill location |

### Repeatable Row Variables

| Variable | Row | Meaning |
| --- | --- | --- |
| `{{ROUTE_SCOPE}}` | Scope Routing | Path, glob, package, service, or work area |
| `{{ROUTE_STANDARDS}}` | Scope Routing | Standards that must be read before editing |
| `{{ROUTE_CWD}}` | Scope Routing | Working directory for verification |
| `{{ROUTE_VERIFY_COMMAND}}` | Scope Routing | Non-interactive verification command |
| `{{ROUTE_NOTES}}` | Scope Routing | Coverage limits, prerequisites, or escalation notes |
| `{{SOURCE_CONCERN}}` | Sources Of Truth | Subject governed by the source |
| `{{SOURCE_REFERENCE}}` | Sources Of Truth | Existing relative path or discovery command |

## Purpose, Scope, And Instruction Precedence

> **Documentation language**: {{DOCUMENT_LANGUAGE}}

{{PROJECT_SUMMARY}}

This guide defines how coding agents work in this repository. It applies to
read-only analysis, documentation, source changes, configuration, verification,
and delivery unless a more specific in-scope guide says otherwise.

{{INSTRUCTION_PRECEDENCE}}

Agents must preserve user-owned changes, state consequential assumptions, and
request direction before expanding beyond the authorized scope.

## Non-Negotiable Gates

### Change Classification

Before editing, classify the requested work using this policy:

{{CHANGE_CLASSIFICATION_POLICY}}

Read-only investigation does not authorize writes. The classification policy
must distinguish editorial documentation from normative/governance
documentation. Source, configuration, dependency, build, deployment, security,
and external-state changes may have different gates; apply the configured
policy literally.

### Approval And Authorization

{{APPROVAL_POLICY}}

Approval for one decision or action does not authorize materially different
work. Record any approval evidence required by the project before proceeding.

<!-- OPTIONAL MODULE: DECISION_RECORDS -->
### Decision Records

- **Policy**: {{DECISION_RECORD_POLICY}}
- **Authoritative location**: `{{DECISION_RECORD_ROOT}}`
- **Approval**: {{DECISION_APPROVAL_POLICY}}
- **Implementation-permitting state**: `{{DECISION_ACCEPTED_STATE}}`
- **Decision states**: {{DECISION_STATUS_POLICY}}
- **Implementation states**: {{IMPLEMENTATION_STATUS_POLICY}}
- **Identity and storage**: {{DECISION_IDENTITY_POLICY}}
- **Provenance**: {{DECISION_PROVENANCE_POLICY}}
- **Stable evidence**: {{DECISION_EVIDENCE_POLICY}}
- **Operational records**: {{OPERATIONAL_CHANGE_RECORD_POLICY}}
- **Operational template**: `{{OPERATIONAL_CHANGE_TEMPLATE_PATH}}`
- **Operational core and extensions**: {{OPERATIONAL_CHANGE_SAFETY_POLICY}}

AI agents and humans may draft or approve a decision record when the configured
policy permits it, but the author or drafting agent may not approve that same
record. The final `Approver` must use the concrete `@<actor-id>` of the actual
actor. Known AI actors use their stable agent ID. For a human approver, run
`id -un` on the execution machine and record the result as
`@<local-login-user>`; do not hard-code a repository account. Generic type and
role labels are invalid. Material uncertainty requires a recorded determination
from a specifically identified human before acceptance. If an approved decision
changes materially, apply the configured re-approval rule before implementation
continues.
<!-- END OPTIONAL MODULE: DECISION_RECORDS -->

## Execution Workflow

1. Read this guide and every more-specific instruction that applies to the
   requested scope.
2. Inspect the relevant code, documentation, build files, and current worktree
   before proposing or making changes.
3. Classify the change. Read-only work needs no task branch. When files will
   change and a version-control policy applies, create the authorized task branch
   before drafting or submitting a required decision record.
4. Draft any required decision record, then satisfy every configured approval
   gate before implementation.
5. Search for existing internal capabilities and choose the smallest coherent
   change that meets the request.
6. Implement only the approved scope and preserve unrelated worktree changes.
7. Run the narrowest relevant non-interactive checks, then the broader checks
   required by the affected routing rows.
8. Report changed files, verification results, known limitations, and stable
   evidence. Do not claim completion while required work remains.

## Scope Routing

Duplicate the row below for every maintained scope. Commands must be
non-interactive, use the stated working directory, and describe their actual
coverage without implying checks they do not perform.

| Scope | Read first | Working directory | Verification command | Notes |
| --- | --- | --- | --- | --- |
| `{{ROUTE_SCOPE}}` | `{{ROUTE_STANDARDS}}` | `{{ROUTE_CWD}}` | `{{ROUTE_VERIFY_COMMAND}}` | {{ROUTE_NOTES}} |

Define a fallback for affected paths that match no routing row. Use the
narrowest discoverable non-interactive check; when none exists, require a
structured review and explicit reporting instead of inventing a command.

<!-- OPTIONAL MODULE: REFERENCE_FRESHNESS -->
### Reference Freshness

{{REFERENCE_FRESHNESS_POLICY}}

Do not create an unrelated documentation diff merely to refresh a review date.
When authoritative sources cannot be reached, use the project's locked local
standard and report the limitation.
<!-- END OPTIONAL MODULE: REFERENCE_FRESHNESS -->

## Global Engineering Rules

- Read existing interfaces and nearby patterns before designing a change.
- Prefer an existing component, utility, client, schema, validator, hook, or
  service when it already satisfies the requirement.
- Keep the diff focused; avoid unrelated refactors, dependency additions, and
  drive-by formatting.
- Add intent-bearing documentation for new public behavior when the applicable
  standards require it.
- Use explicit error handling and structured diagnostics supported by the
  project; do not conceal failures.
- Do not hard-code environment-specific values or modify generated and vendor
  content unless the configured workflow explicitly requires it.

### Security

- Never hard-code or expose secrets, passwords, keys, tokens, or sensitive data.
- Validate external input at trust boundaries.
- Apply least privilege to users, services, credentials, and infrastructure.
- Use approved libraries for cryptography, authentication, and authorization.
- Avoid unsafe command construction, raw query concatenation, and untrusted
  deserialization.

### Version-Control Safety

- Inspect the worktree before changing branches, pulling, staging, or committing.
- Do not discard, overwrite, format, or stage unrelated user-owned changes.
- Stage explicit approved paths and inspect the staged diff before committing.
- Do not run destructive or externally publishing operations without the
  authorization required by the project.

<!-- OPTIONAL MODULE: VERSION_CONTROL_POLICY -->
### Branch, Pull Request, And Commit Policy

- **Protected branches**: {{PROTECTED_BRANCH_POLICY}}
- **Task branches**: {{TASK_BRANCH_POLICY}}
- **Pull requests**: {{PULL_REQUEST_POLICY}}
- **Commits**: {{COMMIT_POLICY}}
<!-- END OPTIONAL MODULE: VERSION_CONTROL_POLICY -->

## Verification And Completion Evidence

- Reproduce a reported failure before fixing it when feasible.
- Run focused checks first and broader checks when shared behavior is affected.
- Use the exact commands and working directories from Scope Routing.
- Report skipped, blocked, or failing checks with their full reason.
- Prefer stable evidence such as commit identifiers, immutable links, symbols,
  headings, and command-result summaries. Treat mutable line numbers as
  supplementary evidence only.
- Completion requires the requested behavior, required documentation, required
  checks, and required evidence—not merely an implementation attempt.

## Sources Of Truth

Duplicate the following row for each authoritative source. Link dynamic facts
rather than copying full service, tool, script, agent, skill, or issue catalogs.

| Concern | Authoritative path or discovery command |
| --- | --- |
| {{SOURCE_CONCERN}} | `{{SOURCE_REFERENCE}}` |

<!-- OPTIONAL MODULE: COLLABORATION_DISCOVERY -->
## Collaboration Discovery

- Agent definitions: `{{AGENT_CONFIG_PATH}}`
- Reusable skills and procedures: `{{SKILL_DIRECTORY}}`

Discover current entries from those locations instead of maintaining duplicate
inventories in this guide.
<!-- END OPTIONAL MODULE: COLLABORATION_DISCOVERY -->

## Developer Principles

1. Take pride in careful research; reject guessing interfaces in the dark.
2. Take pride in seeking confirmation; reject vague execution.
3. Take pride in stating assumptions; reject hiding assumptions.
4. Take pride in reading the code first; reject skipping the reading.
5. Take pride in following existing patterns; reject ignoring established conventions.
6. Take pride in verifying against source code; reject inventing knowledge.
7. Take pride in minimal implementation; reject overengineering.
8. Take pride in implementing functionality directly; reject premature abstraction.
9. Take pride in explaining trade-offs; reject ignoring them.
10. Take pride in recommending the best option; reject listing options without judgment.
11. Take pride in minimal changes; reject broadening the diff.
12. Take pride in focusing on what is required; reject unrelated side changes.
13. Take pride in matching the existing style; reject imposing personal style preferences.
14. Take pride in preserving the original shape; reject casual drive-by formatting.
15. Take pride in reading the full error; reject blind fixes.
16. Take pride in reproducing issues first; reject fixing without reproduction.
17. Take pride in changing one thing and testing one thing; reject mixing many changes and tests.
18. Take pride in proactive testing; reject skipping verification.
19. Take pride in tracing the root cause; reject working around it.
20. Take pride in adding dependencies cautiously; reject adding packages lightly.
21. Take pride in explaining cause and effect; reject posting code without context.
22. Take pride in proactively warning about risks; reject hiding them.
23. Take pride in marking decisions clearly; reject making decisions silently.
24. Take pride in seeking approval first; reject uncontrolled refactoring.
25. Take pride in memorable naming; reject bland, forgettable names.
26. Take pride in short functions; reject long, exhausting ones.
27. Take pride in paying down debt gradually; reject letting decay accumulate.
28. Take pride in consistent style; reject arbitrary coding.
29. Take pride in shipping behind switches; reject confident bare changes.
30. Take pride in easy-to-use interfaces; reject complex ambiguity.
31. Take pride in branch warnings; reject silent neglect.
32. Take pride in monitoring and tuning; reject abandoning maintenance.
33. Take pride in using first-principles thinking to deeply analyze the essence of problems; reject focusing only on surface appearances.
34. Take pride in configuration, reject hardcoding.
35. Take pride in defining variables, reject writing magic values.
36. Take pride in solving problems; reject relying on fallbacks.
