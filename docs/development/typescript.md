# TypeScript Development Standard

**Applies to**: any TypeScript frontend, service, or tool in this repository.

**Last reviewed**: 2026-08-07

## Required Reading

- [The TypeScript Handbook](https://www.typescriptlang.org/docs/handbook/intro.html) —
  the official language and type-system reference.
- [Google TypeScript Style Guide](https://google.github.io/styleguide/tsguide.html) —
  naming, module, type-system, and formatting conventions (named exports
  only, `const`/`let` over `var`, structural typing, avoiding `any`, etc.).

## Baseline Tooling

- Format with the project's configured formatter (for example Prettier) and
  lint with its configured linter (for example ESLint); do not hand-format
  around them.
- All code must pass type checking with the project's configured `tsc`
  settings; do not suppress errors with `@ts-ignore`/`@ts-expect-error`
  without an explanatory comment.

## Before Writing Code

Read this file, then inspect the target package for existing component,
hook, state-management, and API-client patterns, and reuse them instead of
introducing a parallel approach.
