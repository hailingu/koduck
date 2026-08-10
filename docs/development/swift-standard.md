# Swift Development Standard

**Applies to**: any native macOS/iOS (and other Apple platform) app in this
repository built with Apple's Swift, Xcode, and its native app frameworks —
not server-side or cross-platform Swift.

**Last reviewed**: 2026-08-10

## Required Reading

- [Software Engineering Standard](software-engineering-standard.md) — the
  repository-wide baseline for size, module boundaries, dependency direction,
  abstractions, patterns, testing, and exceptions.
- [Apple Developer Documentation](https://developer.apple.com/documentation/) —
  the canonical API reference for Swift and every Apple platform framework
  (SwiftUI, UIKit, AppKit, Foundation, etc.); consult the framework-specific
  page for the APIs actually in use.
- [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines) —
  Apple's official guidance on interaction patterns, layout, typography, and
  platform-specific conventions (macOS vs. iOS/iPadOS); required reading before
  designing or reviewing any UI.
- [SwiftUI Documentation](https://developer.apple.com/documentation/swiftui) —
  the primary UI framework reference; use the UIKit/AppKit integration topics
  when a screen needs a framework-specific view or control.
- [Swift API Design Guidelines](https://www.swift.org/documentation/api-design-guidelines/) —
  the official, canonical naming and API design guidance from swift.org;
  treat this as authoritative for naming, argument labels, and documentation
  comments.
- [Google Swift Style Guide](https://google.github.io/swift/) — a more
  detailed, widely used formatting and construct-level style guide that is
  compatible with and extends the guidelines above.

## Baseline Practices

- Use `swift-format` or `SwiftFormat`/`SwiftLint` (whichever the target
  package already uses) for mechanical formatting instead of hand-formatting.
- Follow the Swift API Design Guidelines for every public declaration's name
  and argument labels; write a documentation comment for every declaration
  before implementing its body.
- Prefer SwiftUI for new UI unless the target already has an established
  UIKit/AppKit codebase; when integrating, follow the platform's own
  Human Interface Guidelines page (macOS and iOS/iPadOS differ).

## Swift Engineering Practices

- Organize application code by user-facing feature or domain capability. Keep
  platform integration, persistence, networking, and shared design-system code
  behind explicit feature boundaries.
- A SwiftUI view owns presentation and local interaction state. Move domain
  policy, persistence, networking, and reusable orchestration out of the view;
  split a large view along independent state, behavior, or reuse boundaries
  rather than arbitrary visual fragments.
- Give each mutable value one state owner. Use bindings for controlled access,
  environment values for genuine scoped dependencies, and actors or main-actor
  isolation where concurrency ownership requires them.
- Introduce protocols for consumer-owned service boundaries or meaningful
  implementation variation. Avoid protocol-and-mock layers for value types and
  direct deterministic logic that can be tested without substitution.
- Prefer structs, enums, and composition. Use class inheritance only for a real
  framework or substitutability requirement, not as the default reuse model.
- Keep DTOs, persistence models, and framework callbacks at adapters; translate
  them into validated feature or domain values before core use.

## Before Writing Code

Read this file, then inspect the target package/app for existing naming
conventions, state ownership, module boundaries, actor isolation, and formatting
configuration (e.g. `.swiftformat`, `.swiftlint.yml`). Match compatible local
conventions and apply the common size and complexity review triggers.
