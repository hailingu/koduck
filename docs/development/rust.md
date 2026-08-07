# Rust Development Standard

**Applies to**: any Rust crate or service in this repository.

**Last reviewed**: 2026-08-07

## Required Reading

- [The Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/) —
  the canonical formatting reference; defer to `rustfmt` (which implements it)
  for mechanical formatting instead of hand-formatting or debating style.
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) — naming,
  interoperability, documentation, and predictability guidance for any public
  crate API.
- [The Rust Programming Language book](https://doc.rust-lang.org/book/) —
  background reading for language features and idioms not covered above.

## Baseline Tooling

- Format with `cargo fmt`; do not hand-format around it.
- Lint with `cargo clippy`; treat new warnings as defects to fix or explicitly
  and narrowly suppress with a comment explaining why.
- Prefer `Result<T, E>` and domain error types for fallible behavior; avoid
  `unwrap()`/`expect()` in production paths unless the invariant is local,
  explicit, and documented.

## Before Writing Code

Read this file, then inspect the target crate (and sibling crates) for
existing state/config patterns, handlers, error types, tracing helpers, and
test fixtures. Reuse or extend what already exists before adding new
abstractions.
