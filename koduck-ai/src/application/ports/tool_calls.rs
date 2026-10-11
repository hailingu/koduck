// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Consumer-owned model Tool-call boundary types serviced through C-5.

use crate::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};

use super::super::tool_projection::{ToolProjection, ToolProjectionSink};
use super::NewItem;

/// One model-originated Tool call exactly as the provider delivered it.
///
/// `name` and `arguments` are untrusted provider content; they never carry
/// authority and are only resolved against configured descriptors by the
/// tool-execution boundary (ADR-0003 TC-02).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelToolCall {
    /// Declared tool name as delivered.
    pub name: String,
    /// Serialized arguments as delivered.
    pub arguments: String,
}

/// The model-bound view of one committed Tool-call result.
///
/// `content` is the bounded committed executor output, a stable
/// denial/failure summary when the call did not produce output, or the stable
/// non-UTF-8 summary bound by the projection sink to an opaque committed
/// success. It is delivered to the model only inside a continuation request
/// started after the current-generation durable result commit the C-5 boundary
/// proved, and it remains untrusted content there (ADR-0003 TC-11).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelToolResult {
    /// Bounded committed result content for the continuation request.
    pub content: String,
    /// Whether the call failed, was denied, or was unavailable.
    pub is_error: bool,
}

/// One serviced Tool call paired with its committed result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedToolCall {
    /// The model-originated call exactly as delivered (untrusted).
    pub call: ModelToolCall,
    /// The committed result carried into the continuation request.
    pub result: ModelToolResult,
}

/// One provider Tool-call round: every call the model raised in one stream,
/// each paired with its committed result.
///
/// Continuation requests carry rounds in order and the provider adapter
/// serializes them as alternating assistant-call/result groups, so a later
/// round raised on an earlier result is never rewritten as concurrent with
/// it (ADR-0003 TC-11).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRound {
    /// Assistant text emitted in this stream before or alongside the Tool
    /// calls. It is retained with the call batch so the continuation can
    /// reconstruct the model's causal assistant message.
    pub assistant_content: String,
    /// The round's serviced calls in the order the model raised them.
    pub calls: Vec<CommittedToolCall>,
}

/// Turn-scoped identity context for one serviced Tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallTurnContext {
    /// Tenant that owns the Turn.
    pub tenant_id: TenantId,
    /// Thread that owns the Turn.
    pub thread_id: ThreadId,
    /// Turn whose D-7 budget the call consumes.
    pub turn_id: TurnId,
    /// Foreground lease generation that must remain current.
    pub lease_generation: LeaseGeneration,
}

/// Consumer-owned boundary that services one model Tool call through C-5 and
/// returns the ordered append-only D-3 items to record for it plus the bounded
/// committed result the runner's continuation request carries.
///
/// The runner owns the durable append-before-publish ordering; the port owns
/// C-5 policy, approval, execution, and the D-3 projection contents. A typed
/// denial or unavailability is returned as recorded items, never as an error.
pub trait ToolCallExecutor {
    /// Services one Tool call and returns its D-3 items and committed result.
    ///
    /// # Errors
    ///
    /// Returns [`ToolCallError`] only for turn-level failures that own the
    /// turn terminal, such as canonical reconciliation or durability.
    fn execute_tool_call(
        &mut self,
        call: ModelToolCall,
        context: &ToolCallTurnContext,
        trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, crate::application::ToolCallError>;

    /// Cancels live C-5 work and returns its canonical D-7 terminal items.
    ///
    /// The default is deliberately a no-op because configurations without a
    /// live C-5 boundary have no process-owned execution work to cancel. The
    /// production boundary overrides it to close catalogued D-7 attempts.
    ///
    /// # Errors
    ///
    /// Returns [`ToolCallError`] when live execution work cannot reach a
    /// canonical terminal and requires reconciliation. Returned items must be
    /// persisted before the Turn interruption terminal.
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<Vec<NewItem>, crate::application::ToolCallError> {
        Ok(Vec::new())
    }

    /// Notifies the boundary that one Turn's durable terminal committed.
    ///
    /// The default is deliberately a no-op because configurations without a
    /// live C-5 boundary retain no process-owned authority. The production
    /// boundary overrides it to reclaim its process-local Turn authority
    /// against the proven canonical terminal; reclamation is hygiene, so an
    /// unproven probe retains the authority instead of surfacing an error.
    fn turn_terminal_committed(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) {
    }
}

/// Explicit unconfigured tool-execution boundary.
///
/// Every call is recorded as a typed unavailability without any execution,
/// caching, or fallback path (ADR-0003 TC-13).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoToolExecution;

impl ToolCallExecutor for NoToolExecution {
    fn execute_tool_call(
        &mut self,
        call: ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, crate::application::ToolCallError> {
        let descriptor_id = if crate::domain::tool::validate_descriptor_id(&call.name).is_ok() {
            call.name
        } else {
            String::new()
        };
        // The unconfigured boundary is recorded through the same durable
        // projection sink as every other outcome (ADR-0003 TC-13).
        crate::application::tool_projection::emit(
            projections,
            ToolProjection::Denied {
                descriptor_id,
                descriptor_version: String::new(),
                target: String::new(),
                code: "tool_execution_unavailable".to_owned(),
            },
        );
        Ok(ModelToolResult {
            content: "tool_execution_unavailable".to_owned(),
            is_error: true,
        })
    }
}
