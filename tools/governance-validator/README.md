# Governance Validator

The validator enforces the repository's deterministic ADD, ADR, and OCR
structure and lifecycle contracts. Run it with the commands documented in
`docs/README.md`.

## Point-In-Time Decomposition Review

Reviewed 2026-08-14 against the current uncommitted task revision. Physical
line counts are review evidence for this revision, not source-to-document
equality assertions. No configured cyclomatic-complexity tool exists; the
review therefore used executable-unit span and nesting as substitute signals.

| Unit | Measured size | Cohesion and decomposition conclusion |
| --- | ---: | --- |
| `validate.mjs` | 794 lines | Below the 800-line exception limit. It owns the CLI traversal, shared Markdown primitives, base lifecycle metadata, and composition of the already-extracted accepted, terminal, relationship, repository-file, and Mermaid validators. The repository-file resolver is separate because canonical containment and regular-file checks form one reusable trust boundary for indexes and record relationships; the remaining shared primitives stay here to avoid a dependency cycle or a generic helper module with no independent lifecycle. |
| `lib/accepted-records.mjs` | 565 lines | Below the 800-line exception limit. It owns one accepted-stage validation boundary and shares parsed subtask/check IDs across touchpoint, traceability, and risk checks. OCR and ADR branches are explicit and do not share mutable state; further splitting would duplicate context plumbing without an independent failure boundary. |
| `test/validate-lifecycle.test.mjs` | 1,077 lines | Below the 1,200-line exception limit. It is the terminal and approval-lifecycle contract suite; its cases reuse the same repository fixtures and each mutation isolates one status or evidence invariant. Splitting the fixture lifecycle would duplicate large normative record samples. |
| `test/validate-structure.test.mjs` | 1,165 lines | Below the 1,200-line exception limit. It is the pre-lifecycle repository-structure suite covering required headings, reciprocal links, IDs, and Mermaid membership through the same fixture graph. Index-path and filesystem trust-boundary cases remain in their focused sibling test file. |

Executable units above 60 physical lines are `validateStatus`, the top-level
`validate` orchestration, `validateAcceptedRiskMatrix`, and the three complete
record fixture builders `completeOcr`, `acceptedOcr`, and `acceptedAdr`.
`validateStatus` and `validate` retain one ordered lifecycle/traversal pass;
extracting individual branches would split shared error accumulation and record
classification. `validateAcceptedRiskMatrix` retains one header/row/reference
pass so column classification cannot drift between helpers. The fixture
builders are declarative complete-record samples whose fields must remain
visible together for review. Each unit remains below the 120-line exception
limit and has executable nesting below five levels. No engineering exception
is required.
