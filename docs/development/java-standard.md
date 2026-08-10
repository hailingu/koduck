# Java Development Standard

**Applies to**: any Java service in this repository.

**Last reviewed**: 2026-08-10

## Required Reading

- [Software Engineering Standard](software-engineering-standard.md) — the
  repository-wide baseline for size, module boundaries, dependency direction,
  abstractions, patterns, testing, and exceptions.
- [Google Java Style Guide](https://google.github.io/styleguide/javaguide.html) —
  the canonical formatting, naming, and Javadoc reference used for Java code
  in this repository.

## Baseline Practices

- Format with the project's configured formatter (for example
  `google-java-format`); do not hand-format around it.
- Use `@Override` wherever legal; write Javadoc for every visible class,
  member, and record component per the style guide's Javadoc section.

## Java Engineering Practices

- Organize packages by business capability. Within a capability, separate
  application orchestration, domain policy, persistence, and delivery adapters
  only where their dependencies or lifecycles differ.
- Keep Spring, Jakarta, database, serialization, and transport annotations at
  adapter boundaries. Domain types and rules must not require a framework
  container to construct or test them.
- Introduce an interface for an external boundary, meaningful alternative
  implementations, or a consumer-owned test seam. Do not create an interface
  and factory for every service class by convention.
- Prefer constructor injection and immutable fields. Avoid service locators,
  field injection, mutable static state, and inheritance used only to share
  implementation.
- Use records and value types for immutable data with clear invariants; do not
  expose persistence entities as public API models.
- Split a class by responsibility before creating broad `Manager`, `Helper`, or
  `Util` types. Package-private declarations are preferred when no external
  consumer exists.

## Before Writing Code

Read this file, then inspect the target service for existing package
layout, exception hierarchies, dependency direction, framework boundaries, and
test conventions. Match compatible local conventions and apply the common size
and complexity review triggers.
