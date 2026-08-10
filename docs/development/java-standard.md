# Java Development Standard

**Applies to**: any Java service in this repository.

**Last reviewed**: 2026-08-07

## Required Reading

- [Google Java Style Guide](https://google.github.io/styleguide/javaguide.html) —
  the canonical formatting, naming, and Javadoc reference used for Java code
  in this repository.

## Baseline Practices

- Format with the project's configured formatter (for example
  `google-java-format`); do not hand-format around it.
- Use `@Override` wherever legal; write Javadoc for every visible class,
  member, and record component per the style guide's Javadoc section.

## Before Writing Code

Read this file, then inspect the target service for existing package
layout, exception hierarchies, and test conventions, and match them.
