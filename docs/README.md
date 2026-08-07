# Koduck Documentation

This is the single navigation entry point for documentation under `docs/`.
The root [`AGENTS.md`](../AGENTS.md) remains authoritative for governance,
decision-record gates, approval, evidence, version control, and delivery rules.
Do not duplicate those rules in directory-local README files.

## Decision Records

- [`adr/INDEX.md`](adr/INDEX.md) is the single repository-wide index for every
  active and archived project or service ADR and OCR.
- Full and Lightweight project ADRs live directly in `docs/adr/`.
- Project OCRs live in `docs/adr/ocr/`.
- Future service records retain their service-local ADR/OCR paths but add their
  single catalog row to `docs/adr/INDEX.md`.
- ADR and OCR prefixes, directories, and number sequences remain distinct even
  though their status rows share one index.
- Archived records move into the matching `archive/` directory; the index row
  is updated rather than deleted.

Templates:

- [Full ADR](adr/template/0000-template.md)
- [Lightweight ADR](adr/template/0000-lightweight-template.md)
- [OCR](adr/template/0000-operational-change-template.md)

Read the Decision Records section of [`AGENTS.md`](../AGENTS.md) before drafting,
approving, implementing, operating, or archiving a record.

## Development Standards

Before writing, modifying, or reviewing source code or infrastructure
configuration, read this catalog and every matching language or platform file
in full. When work spans multiple languages or platforms, read every applicable
file.

| File | Language / platform |
| --- | --- |
| [development/rust.md](development/rust.md) | Rust |
| [development/swift.md](development/swift.md) | Swift |
| [development/python.md](development/python.md) | Python |
| [development/typescript.md](development/typescript.md) | TypeScript |
| [development/java.md](development/java.md) | Java |
| [development/kubernetes.md](development/kubernetes.md) | Kubernetes manifests and operations |

Each standard applies to every current or future service, package, or script
using that language or platform. Inspect the affected module for established
local conventions and use its configured formatter, linter, and non-interactive
checks. A local convention may preserve consistency within its module unless it
conflicts with an Accepted decision or another binding requirement.

### Reference Freshness

Each standard records a `Last reviewed` date. It may be used without
revalidation when that date is within 180 days and its authoritative source has
not had a breaking revision. Otherwise, revalidate the source before relying on
it for a non-trivial change and update `Last reviewed`. When offline, use the
locked local content and report the limitation instead of inventing guidance.
