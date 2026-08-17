// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Bounded envelope and response types crossing the isolated executor port.

/// Maximum buffered byte size for one isolated executor response.
pub const MAX_EXECUTOR_OUTPUT_BYTES: usize = 1_048_576;

/// Executor-observed state of an external effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectState {
    /// The executor proves that no effect started.
    NotStarted,
    /// The executor observed that the effect started.
    Started,
    /// The executor cannot prove whether the effect started.
    Unknown,
}

/// A stable failure emitted by the C-5 execution boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionFailure {
    /// The configured isolated executor is unavailable.
    ExecutorUnavailable,
    /// The owner was fenced before executor dispatch and no terminal write won.
    OwnerFencedBeforeDispatch,
    /// The owner was fenced after dispatch and no result may reach the model.
    OwnerFencedAfterDispatch,
    /// The isolated result exceeded 1,048,576 serialized bytes.
    OutputLimitExceeded,
    /// D-6 does not authorize this exact binding.
    ApprovalMismatch,
    /// The canonical D-7 already claimed its only dispatch.
    ApprovalAlreadyConsumed,
    /// An authenticated interruption sealed the Turn before the dispatch claim.
    InterruptionRequested,
    /// The canonical result could not be committed durably.
    DurabilityUnavailable,
    /// A different canonical terminal won the conditional commit race.
    TerminalConflict,
    /// Another D-7 owns this Turn's single running slot.
    ConcurrentAttempt,
    /// The current foreground lease generation could not be validated;
    /// ownership is undetermined and reconciliation owns the next transition.
    LeaseUnavailable,
    /// The Turn's 16-slot D-7 attempt budget is exhausted.
    AttemptLimit,
    /// The addressed D-7 is not running, so no bounded cancellation exists.
    AttemptNotRunning,
}

impl ExecutionFailure {
    /// Returns the stable D-3 terminal code for this failure.
    #[must_use]
    pub const fn stable_code(self) -> &'static str {
        match self {
            Self::ExecutorUnavailable => "executor_unavailable",
            Self::OwnerFencedBeforeDispatch => "owner_fenced_before_dispatch",
            Self::OwnerFencedAfterDispatch => "owner_fenced_after_dispatch",
            Self::OutputLimitExceeded => "output_limit_exceeded",
            Self::ApprovalMismatch => "approval_mismatch",
            Self::ApprovalAlreadyConsumed => "approval_already_consumed",
            Self::InterruptionRequested => "interruption_requested",
            Self::DurabilityUnavailable => "durability_unavailable",
            Self::TerminalConflict => "terminal_conflict",
            Self::ConcurrentAttempt => "concurrent_attempt",
            Self::LeaseUnavailable => "lease_unavailable",
            Self::AttemptLimit => "attempt_limit",
            Self::AttemptNotRunning => "attempt_not_running",
        }
    }
}

/// Executor failure paired with truthful external-effect evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutorError {
    code: ExecutionFailure,
    effect_state: EffectState,
}

impl ExecutorError {
    /// Creates a failure whose effect state was observed by the executor boundary.
    #[must_use]
    pub const fn new(code: ExecutionFailure, effect_state: EffectState) -> Self {
        Self { code, effect_state }
    }

    /// Returns the stable failure code observed at the executor boundary.
    #[must_use]
    pub const fn code(&self) -> ExecutionFailure {
        self.code
    }

    /// Returns the executor's observed effect-state evidence.
    #[must_use]
    pub const fn effect_state(&self) -> EffectState {
        self.effect_state
    }
}

/// One bounded response from the isolated executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionResponse {
    effect_state: EffectState,
    output: Vec<u8>,
}

impl ExecutionResponse {
    /// Returns executor evidence about whether the external effect started.
    #[must_use]
    pub const fn effect_state(&self) -> EffectState {
        self.effect_state
    }

    /// Returns the opaque bounded output for conditional durable commit.
    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Consumes the response and returns its bounded output for the commit.
    #[must_use]
    pub fn into_output(self) -> Vec<u8> {
        self.output
    }
}

/// Incremental constructor that enforces the executor output cap before buffering.
#[derive(Debug)]
pub struct ExecutionResponseBuilder {
    effect_state: EffectState,
    output: Vec<u8>,
    overflowed: bool,
}

impl ExecutionResponseBuilder {
    /// Starts an empty response with the executor's observed effect state.
    #[must_use]
    pub const fn new(effect_state: EffectState) -> Self {
        Self {
            effect_state,
            output: Vec::new(),
            overflowed: false,
        }
    }

    /// Appends one transport chunk without allowing the buffer to exceed 1,048,576 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionFailure::OutputLimitExceeded`] before appending any chunk that
    /// would cross the response limit.
    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), ExecutorError> {
        if self.overflowed
            || chunk.len() > MAX_EXECUTOR_OUTPUT_BYTES.saturating_sub(self.output.len())
        {
            self.overflowed = true;
            return Err(ExecutorError::new(
                ExecutionFailure::OutputLimitExceeded,
                self.effect_state,
            ));
        }
        self.output.extend_from_slice(chunk);
        Ok(())
    }

    /// Finishes the already bounded response for coordinator commitment.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionFailure::OutputLimitExceeded`] when any prior chunk
    /// crossed the response limit, even if the caller ignored that append error.
    pub fn finish(self) -> Result<ExecutionResponse, ExecutorError> {
        if self.overflowed {
            return Err(ExecutorError::new(
                ExecutionFailure::OutputLimitExceeded,
                self.effect_state,
            ));
        }
        Ok(ExecutionResponse {
            effect_state: self.effect_state,
            output: self.output,
        })
    }
}
