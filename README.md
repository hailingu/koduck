# Koduck

Koduck is a from-scratch rebuild of `koduck-quant`, restructured to fix
governance and repository-organization problems found during that project's
development — most importantly, ambiguous ADR/OCR naming, missing ADR
archival, and unclear project-level vs. service-level decision-record scope.

The repository now contains the first service, `koduck-ai`, alongside the
governance scaffolding, decision-record process, templates, and per-language
development standards used to evolve it.

## Start Here

- [AGENTS.md](AGENTS.md) — the authoritative guide for how agents (and
  contributors) work in this repository: change classification, decision
  records, branch/PR/commit policy, and verification requirements.
- [CLAUDE.md](CLAUDE.md) — a thin entry point that defers to `AGENTS.md`.
- [docs/README.md](docs/README.md) — the navigation entry point for
  everything under `docs/`; read it before drafting documents or writing code.
- [docs/architecture/](docs/architecture/) — Architecture Design Documents
  (ADDs). See [docs/architecture/INDEX.md](docs/architecture/INDEX.md) for the
  status of every ADD.
- [docs/adr/](docs/adr/) — project-level Architecture Decision Records (ADRs)
  and Operational Change Records (OCRs). See [docs/adr/INDEX.md](docs/adr/INDEX.md)
  for the current, fast-scan status of every record.
- [docs/development/](docs/development/) — a catalog of authoritative,
  real-world reference documentation for each language/platform this
  repository expects to use (Rust, Swift, Python, TypeScript, Java,
  Kubernetes). Read the matching file before writing code in that language.
- [docs/delivery/](docs/delivery/) — release and Git tag standards. Read both
  files before planning a release or tag operation.

## Local SonarQube gate

Install with `sh tools/sonarqube/install.sh`. Both Git hooks use
`scripts/sonar-quality-gate.sh`, adapted from the PlotWeave gate. Koduck
authentication uses `KODUCK_SONAR_TOKEN` exported by `~/.zshrc`; no credential
is stored in this repository. See [the workflow guide](tools/sonarqube/README.md)
for the exact staged-snapshot, incremental-issue, coverage and CI contract.

## Repository Structure

```text
AGENTS.md              Agent/contributor guide and governance rules
CLAUDE.md              Claude-specific entry point (defers to AGENTS.md)
Cargo.toml             Rust workspace manifest
koduck-ai/              Provider-neutral AI turn service and its contracts
docs/
  architecture/        Architecture Design Documents (ADDs), template, and index
  adr/                 Project-level ADRs, OCRs (in ocr/), templates, and indexes
  delivery/            Release and Git tag standards
  development/         Per-language development standards catalog
LICENSE                MIT License
```

Each service added to this repository is expected to maintain its own
`<service>/docs/adr/` for decisions scoped to that service alone; see
`AGENTS.md`'s "Project vs. Service ADR/OCR Routing" for how to decide where a
given decision belongs.

## License

MIT — see [LICENSE](LICENSE).
