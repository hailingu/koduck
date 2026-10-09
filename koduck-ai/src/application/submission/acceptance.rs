// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! Consumer-owned monotonic acceptance clock and per-request budget values
//! (SI-07i, SI-07c, SI-07d).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// The exact identified-request acceptance deadline (SI-07d).
pub const ACCEPTANCE_DEADLINE: Duration = Duration::from_secs(10);
/// The exact maximum pre-write read budget, clamped to remaining acceptance
/// time (SI-07c).
pub const LOOKUP_BUDGET: Duration = Duration::from_secs(2);
/// The exact write-start gate: a fresh acceptance write may start only with at
/// least this much acceptance time remaining (SI-07d).
pub const WRITE_GATE_MINIMUM: Duration = Duration::from_secs(4);
/// The exact full reserved budget of one acceptance write attempt (SI-07c).
pub const WRITE_BUDGET: Duration = Duration::from_secs(2);
/// The exact full reserved budget of the single read-only reconciliation
/// attempt (SI-07c).
pub const RECONCILIATION_BUDGET: Duration = Duration::from_secs(2);

/// One reading of the chosen acceptance clock in its origin domain (SI-07i).
///
/// The value wraps an owned elapsed duration, so ordering and subtraction stay
/// inside one clock's domain and no wall-clock time enters the policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AcceptanceInstant(Duration);

impl AcceptanceInstant {
    /// Wraps one immediate, nondecreasing clock reading.
    #[must_use]
    pub const fn from_elapsed(elapsed: Duration) -> Self {
        Self(elapsed)
    }

    /// Returns the wrapped elapsed reading.
    #[must_use]
    pub const fn as_elapsed(self) -> Duration {
        self.0
    }
}

/// A consumer-owned monotonic acceptance clock (SI-07i).
///
/// Readings MUST be immediate, nonblocking, and nondecreasing in this clock's
/// domain. The production implementation measures real monotonic elapsed time;
/// tests substitute a manual clock through this port without altering any
/// real database, lease, or fenced-recovery timer.
pub trait AcceptanceClock: Send + Sync {
    /// Returns one immediate clock reading.
    fn now(&self) -> AcceptanceInstant;
}

/// The production system acceptance clock.
///
/// The owned origin is captured once at construction; every reading is the
/// real monotonic elapsed time since that origin, including synchronous
/// preparation and `block_on` waits (SI-07i).
#[derive(Clone, Copy, Debug)]
pub struct SystemAcceptanceClock {
    origin: std::time::Instant,
}

impl SystemAcceptanceClock {
    /// Captures the clock origin at construction.
    #[must_use]
    pub fn start() -> Self {
        Self {
            origin: std::time::Instant::now(),
        }
    }
}

impl AcceptanceClock for SystemAcceptanceClock {
    fn now(&self) -> AcceptanceInstant {
        AcceptanceInstant::from_elapsed(self.origin.elapsed())
    }
}

/// Returns the shared system acceptance clock behind the consuming port.
#[must_use]
pub fn system_clock() -> Arc<dyn AcceptanceClock> {
    Arc::new(SystemAcceptanceClock::start())
}

/// One identified request's acceptance budget (SI-07i).
///
/// Each request owns a separate start reading and deadline; runner cloning and
/// Tool composition retain the clock dependency while every request derives
/// its own budget.
#[derive(Clone, Copy, Debug)]
pub struct AcceptanceBudget {
    started_at: AcceptanceInstant,
}

impl AcceptanceBudget {
    /// Starts one request's budget from the immediate reading taken at fully
    /// validated input/trust entry (SI-07d).
    #[must_use]
    pub const fn start(started_at: AcceptanceInstant) -> Self {
        Self { started_at }
    }

    /// Returns the remaining acceptance time using checked, bounded
    /// arithmetic, or `None` when the reading precedes the request start
    /// (fail closed without resetting or extending the budget, SI-07i).
    #[must_use]
    pub fn remaining(&self, now: AcceptanceInstant) -> Option<Duration> {
        let elapsed = now.as_elapsed().checked_sub(self.started_at.as_elapsed())?;
        Some(ACCEPTANCE_DEADLINE.saturating_sub(elapsed))
    }

    /// Clamps one attempt's maximum budget to the remaining acceptance time
    /// (SI-07c). Pre-write reads use this clamp; a permitted write and its
    /// reconciliation instead receive their full reserved budgets.
    #[must_use]
    pub fn clamp(&self, attempt: Duration, now: AcceptanceInstant) -> Option<Duration> {
        let remaining = self.remaining(now)?;
        Some(attempt.min(remaining))
    }
}

impl fmt::Display for AcceptanceInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self.0)
    }
}
