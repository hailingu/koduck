# Repository Decision Record Index

This is the single index for every project or service ADR and OCR, active or
archived. Update a row whenever its type, title, Decision Status,
Implementation Status, scope, Architecture Source, path, or supersession
metadata changes. Never delete a row during archival; update its status and
`Path`. The record itself remains authoritative for its content.

| Type | ID | Title | Decision Status | Implementation Status | Scope | Architecture Source | Path | Superseded By |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Full ADR | ADR-0001 | Provider-Neutral Tool-Free Turn Kernel | Accepted | Complete | Project | `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-1 | `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` | None |
| Full ADR | ADR-0002 | Required Koduck AI CI And PostgreSQL Verification | Accepted | Verified | Project | N/A — this is repository verification governance, not product demand | `docs/adr/ADR-0002-required-ai-ci-postgres-verification.md` | None |
| Full ADR | ADR-0003 | Default-Deny Tool Approval And Execution Boundary | Accepted | Complete | Project | `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-2 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` | None |
| Full ADR | ADR-0004 | Provider Stream Completion Normalization | Accepted | Complete | Project | N/A — this corrective provider-protocol compatibility task was discovered through local verification of the existing provider-neutral runtime and is not derived from a new Trello product requirement | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` | None |
| Full ADR | ADR-0005 | Provider Delta Coalescing And 512-Item Turn Budget | Accepted | Complete | Project | N/A — this corrective provider-fragmentation compatibility task was discovered through local verification of the existing provider-neutral runtime and is not derived from a new Trello product requirement | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` | None |
| Full ADR | ADR-0007 | Linear-Time Governance Path Recognition | Accepted | Complete | Project | N/A — corrective security work requested against the local SonarQube project, not derived from product demand | `docs/adr/ADR-0007-linear-time-governance-path-recognition.md` | None |
| Full ADR | ADR-0008 | Delimiter-Bounded Governance Record Paths | Accepted | In Progress | Project | N/A — corrective governance-validator work requested from the local SonarQube result, not derived from product demand | `docs/adr/ADR-0008-delimiter-bounded-governance-record-paths.md` | None |
| Full ADR | ADR-0001 | Strict JSON Duplicate-Member Validation | Accepted | Complete | Service internal — koduck-ai | N/A — corrective internal maintainability work discovered through source review, not derived from product demand | `koduck-ai/docs/adr/ADR-0001-strict-json-duplicate-member-validation.md` | None |
| Lightweight ADR | ADR-0002 | Typed HTTP Wire Serialization | Accepted | Complete | Service internal — koduck-ai | N/A — corrective internal maintainability work discovered through source review, not derived from product demand | `koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md` | None |
| Full ADR | ADR-0003 | Correction Item Schema and Raw Replay | Accepted | Complete | Service internal — koduck-ai | `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-3 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` | None |
| OCR | OCR-0001 | Runtime Dependency Lock Generation | Accepted | Complete | Project | `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`, subtask T-3 | `docs/adr/ocr/archive/OCR-0001-runtime-dependency-lock-generation.md` | None |
| OCR | OCR-0002 | Dev Required Koduck AI Checks | Accepted | Verified | Project | `docs/adr/ADR-0002-required-ai-ci-postgres-verification.md`, subtask T-3 | `docs/adr/ocr/archive/OCR-0002-dev-required-ai-checks.md` | None |
| OCR | OCR-0003 | Local SonarQube Security Remediation Verification | Accepted | Complete | Project | `docs/adr/ADR-0007-linear-time-governance-path-recognition.md`, subtask T-3 | `docs/adr/ocr/archive/OCR-0003-local-sonarqube-security-remediation-verification.md` | None |
| OCR | OCR-0004 | Local SonarQube Reliability Remediation Verification | Proposed | Not Started | Project | `docs/adr/ADR-0008-delimiter-bounded-governance-record-paths.md`, subtask T-3 | `docs/adr/ocr/OCR-0004-local-sonarqube-reliability-remediation-verification.md` | None |
