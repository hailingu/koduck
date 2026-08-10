# Python Development Standard

**Applies to**: any Python package, service, or script in this repository.

**Last reviewed**: 2026-08-10

## Required Reading

- [Software Engineering Standard](software-engineering-standard.md) — the
  repository-wide baseline for size, module boundaries, dependency direction,
  abstractions, patterns, testing, and exceptions.
- [PEP 8 — Style Guide for Python Code](https://peps.python.org/pep-0008/) —
  the canonical language style guide; project-specific conventions in this
  document take precedence only where they narrow, not contradict, PEP 8.
- [PEP 257 — Docstring Conventions](https://peps.python.org/pep-0257/) —
  canonical docstring formatting and placement rules.
- [Google Python Style Guide](https://google.github.io/styleguide/pyguide.html) —
  a more detailed style guide (linting, typing, exceptions, naming) that is
  compatible with and extends PEP 8/257.

## Baseline Practices

- Format and lint with the tool the target package already uses (for example
  `ruff`, `black`, or `pylint`); do not hand-format around it.
- Use type hints ([PEP 484](https://peps.python.org/pep-0484/)) on public
  functions and methods.
- Write a docstring for every public module, class, function, and method.

## Python Engineering Practices

- Organize packages by capability and keep `__init__.py` focused on an
  intentional public API. Do not hide substantial execution, registration, or
  mutable initialization in package imports.
- Keep framework handlers, ORM models, serializers, and provider clients at
  boundaries. Domain rules should accept and return domain values rather than
  framework request, response, or persistence objects.
- Use a `Protocol` or abstract base class for a consumer-owned boundary or
  meaningful implementation variation, not to wrap every concrete class.
- Prefer small functions and explicit composition. Use classes when state,
  invariants, polymorphism, or lifecycle ownership justify them; do not turn a
  collection of stateless functions into a class by convention.
- Import cycles and generic `utils.py` modules are prohibited. Avoid mutable
  module globals and wildcard imports. Name shared modules after the capability
  they own.
- Keep synchronous and asynchronous APIs distinct. Do not block an event loop
  with synchronous I/O or expose a coroutine boundary without defining
  cancellation and timeout behavior.

## Before Writing Code

Read this file, then inspect the target package for existing import
ordering, package boundaries, error-handling patterns, logging setup, async
model, and test fixtures. Match compatible local conventions and apply the
common size and complexity review triggers.
