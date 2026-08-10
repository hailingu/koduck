# Python Development Standard

**Applies to**: any Python package, service, or script in this repository.

**Last reviewed**: 2026-08-07

## Required Reading

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

## Before Writing Code

Read this file, then inspect the target package for existing import
ordering, error-handling patterns, logging setup, and test fixtures, and
match them.
