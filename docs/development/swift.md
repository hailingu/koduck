# Swift Development Standard

**Applies to**: any native macOS/iOS (and other Apple platform) app in this
repository built with Apple's Swift, Xcode, and its native app frameworks —
not server-side or cross-platform Swift.

**Last reviewed**: 2026-08-07

## Required Reading

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

## Baseline Tooling

- Use `swift-format` or `SwiftFormat`/`SwiftLint` (whichever the target
  package already uses) for mechanical formatting instead of hand-formatting.
- Follow the Swift API Design Guidelines for every public declaration's name
  and argument labels; write a documentation comment for every declaration
  before implementing its body.
- Prefer SwiftUI for new UI unless the target already has an established
  UIKit/AppKit codebase; when integrating, follow the platform's own
  Human Interface Guidelines page (macOS and iOS/iPadOS differ).

## Before Writing Code

Read this file, then inspect the target package/app for existing naming
conventions, module boundaries, and formatting configuration (e.g.
`.swiftformat`, `.swiftlint.yml`) and match them.
