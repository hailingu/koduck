# Koduck

Koduck is a from-scratch rebuild of `koduck-quant`, restructured to fix
governance and repository-organization problems found during that project's
development — most importantly, ambiguous ADR/OCR naming, missing ADR
archival, and unclear project-level vs. service-level decision-record scope.

No service has been added to this repository yet. Its current content is the
governance scaffolding — decision-record process, templates, and per-language
development standards — that future services will build on.

## Start Here

- [AGENTS.md](AGENTS.md) — the authoritative guide for how agents (and
  contributors) work in this repository: change classification, decision
  records, branch/PR/commit policy, and verification requirements.
- [CLAUDE.md](CLAUDE.md) — a thin entry point that defers to `AGENTS.md`.
- [docs/adr/](docs/adr/) — project-level Architecture Decision Records (ADRs)
  and Operational Change Records (OCRs). See [docs/adr/INDEX.md](docs/adr/INDEX.md)
  for the current, fast-scan status of every record.
- [docs/development/](docs/development/) — a catalog of authoritative,
  real-world reference documentation for each language/platform this
  repository expects to use (Rust, Swift, Python, TypeScript, Java,
  Kubernetes). Read the matching file before writing code in that language.

## Repository Structure

```text
AGENTS.md              Agent/contributor guide and governance rules
CLAUDE.md              Claude-specific entry point (defers to AGENTS.md)
docs/
  adr/                 Project-level ADRs, OCRs (in ocr/), templates, and indexes
  development/         Per-language development standards catalog
LICENSE                MIT License
```

Each service added to this repository is expected to maintain its own
`<service>/docs/adr/` for decisions scoped to that service alone; see
`AGENTS.md`'s "Project vs. Service ADR/OCR Routing" for how to decide where a
given decision belongs.

## License

MIT — see [LICENSE](LICENSE).
