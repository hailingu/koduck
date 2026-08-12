<!-- ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md -->

# Koduck AI Runtime Configuration

This document is the configuration schema for the CAND-1 executable. The
Accepted ADR remains authoritative when this implementation copy differs.

## Required Environment

| Variable | Format | Purpose | Sensitive |
| --- | --- | --- | --- |
| `KODUCK_AI_BIND_ADDR` | Socket address such as `127.0.0.1:8080` | Axum listener address | No |
| `KODUCK_AI_DATABASE_URL` | PostgreSQL connection URL | Canonical Thread, Turn, Item, and lease storage | Yes |
| `KODUCK_AI_OPENAI_BASE_URL` | HTTPS base URL ending before `/chat/completions` | Explicit OpenAI-compatible provider | No |
| `KODUCK_AI_OPENAI_MODEL` | Non-empty provider model identifier | Tool-free chat-completions model | No |
| `KODUCK_AI_OPENAI_API_KEY` | Provider bearer credential | Provider authentication | Yes |

Every variable is required and blank values are rejected. Runtime debug output
redacts both the database URL and provider credential. Operators must supply
secrets through the deployment's approved secret mechanism; they must not be
committed, logged, or placed in Trello or review evidence.

## Validated Identity Handoff

The Axum boundary accepts `X-Koduck-Tenant-Id` and `X-Koduck-Subject-Id` only as
identity already validated by the configured gateway/Auth boundary. The
gateway must remove caller-supplied values and set both headers from its
validated identity. A deployment must prevent direct untrusted access to the
AI listener; that topology and its verification require an Accepted OCR.

Missing, blank, or non-UTF-8 identity values produce the owned `401
invalid-identity` response before the turn runner, provider, or history ports
are called.

## Startup

The executable connects to PostgreSQL and applies the idempotent CAND-1 schema,
with each startup operation limited by the 2-second database-attempt deadline,
constructs exactly one `PostgresTurnHistory<SqlxPostgresExecutor>`, constructs
the configured OpenAI-compatible transport, binds the listener, and exposes
only the three owned v1 routes. Startup fails explicitly if configuration,
PostgreSQL, provider-client construction, listener binding, or HTTP serving
fails. No process-local, Memory, Multitask, predecessor, or alternate history
fallback is configured.

## Operational Bounds

- Provider connection establishment is limited to 5 seconds, response headers
  to 30 seconds, inactivity between response body chunks to 30 seconds, and
  total response processing to 120 seconds. A deadline produces a provider
  failure and closes the accepted Turn through the normal terminal path.
- Every synchronous PostgreSQL operation uses the approved 2-second attempt
  deadline and maps expiration to `durability-unavailable`.
- Lease-renewal and failed-append recovery tasks share admission for at most
  256 background workers per production `PostgresTurnHistory` instance.
  On an append outage, the Turn stops its renewal worker and waits for that
  worker to release its permit before scheduling recovery, so recovery can
  inherit confirmed capacity even when all 256 slots were occupied. The wait
  remains bounded by the renewal database attempt's 2-second deadline. Other
  saturation rejects new work with `durability-unavailable` instead of
  creating another operating-system thread.
