# Rust Development Standard

**Applies to**: any Rust crate or service in this repository.

**Last reviewed**: 2026-08-10

## Required Reading

- [Software Engineering Standard](software-engineering-standard.md) — the
  repository-wide baseline for size, module boundaries, dependency direction,
  abstractions, patterns, testing, and exceptions.
- [The Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/) —
  the canonical formatting reference; defer to `rustfmt` (which implements it)
  for mechanical formatting instead of hand-formatting or debating style.
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) — naming,
  interoperability, documentation, and predictability guidance for any public
  crate API.
- [The Rust Programming Language book](https://doc.rust-lang.org/book/) —
  background reading for language features and idioms not covered above.

## Baseline Practices

- Format with `cargo fmt`; do not hand-format around it.
- Lint with `cargo clippy`; treat new warnings as defects to fix or explicitly
  and narrowly suppress with a comment explaining why.
- Prefer `Result<T, E>` and domain error types for fallible behavior; avoid
  `unwrap()`/`expect()` in production paths unless the invariant is local,
  explicit, and documented.

## Rust Engineering Practices

- Give each crate one coherent capability and keep its public surface narrow.
  Split crates for ownership, compilation, reuse, or dependency boundaries, not
  merely to reduce file size.
- Organize modules by domain or feature. Keep `lib.rs` and `mod.rs` focused on
  declarations and intentional re-exports rather than orchestration or hidden
  initialization.
- Define traits at the consuming boundary when multiple implementations,
  external I/O, or test substitution requires them. Do not introduce a trait
  only to mirror every inherent implementation.
- Keep transport, database, serialization, and provider types in adapters.
  Model validated domain identifiers and invariants with enums and newtypes
  instead of passing primitive strings through the core.
- Prefer explicit ownership and message passing over shared mutable state.
  When shared state is necessary, document lock ownership, ordering, contention,
  cancellation, and poison or failure behavior.
- Error enums belong to the layer that can interpret the failure. Preserve
  sources when adding context and avoid a single unstructured error variant for
  unrelated failure classes.

## Before Writing Code

Read this file, then inspect the target crate (and sibling crates) for
existing state/config patterns, handlers, error types, tracing helpers, and
test fixtures. Reuse or extend what already exists before adding new
abstractions, preserve the intended crate dependency direction, and apply the
common size and complexity review triggers.
