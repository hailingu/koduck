// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Provider-neutral lifecycle orchestration and durable-before-visible ordering.

use crate::domain::{Item, TenantId, TerminalOutcome, TrustContext, Turn, TurnId, Usage};

use super::ports::{
    AcceptedTurn, ModelInput, ModelProvider, NewItem, ProviderEvent, TurnCommand, TurnHistory,
    TurnResult, TurnRunError,
};

/// Owns provider-neutral lifecycle transitions and durable-before-visible ordering.
pub struct TurnRunner<P, H> {
    provider: P,
    history: H,
}

struct ExecutionState {
    published: Vec<Item>,
    usage: Usage,
    lifecycle: Turn,
}

impl ExecutionState {
    fn started() -> Self {
        Self {
            published: Vec::new(),
            usage: Usage::zero(),
            lifecycle: Turn::start(),
        }
    }
}

impl<P, H> TurnRunner<P, H>
where
    P: ModelProvider,
    H: TurnHistory,
{
    /// Creates a runner from consumer-owned provider and history ports.
    #[must_use]
    pub const fn new(provider: P, history: H) -> Self {
        Self { provider, history }
    }

    /// Records an interrupt request through the canonical history boundary.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError::History`] when the turn is unknown, non-owned,
    /// already terminal, fenced, or the durable store is unavailable.
    pub fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<(), TurnRunError> {
        self.history.request_interrupt(trust, turn_id)?;
        Ok(())
    }

    /// Executes one accepted turn and publishes only successfully appended items.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError`] when initial acceptance, provider setup, append,
    /// replay, or an internal lifecycle transition fails.
    pub fn execute(&mut self, command: TurnCommand) -> Result<TurnResult, TurnRunError> {
        let prior_history = command
            .thread_id
            .map(|thread_id| {
                self.history
                    .prior_thread_items(&command.trust.tenant_id, thread_id)
            })
            .transpose()?
            .unwrap_or_default();
        let accepted = self.history.accept_initial(&command)?;
        let input = ModelInput {
            tenant_id: command.trust.tenant_id.clone(),
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
            input: command.input,
            history: prior_history,
        };
        let mut stream = self.provider.stream(input)?;
        let mut state = ExecutionState::started();
        let mut reached_terminal = false;

        for event in &mut *stream {
            if handle_event(&mut self.history, &accepted, &mut state, event)? {
                reached_terminal = true;
                break;
            }
            if self.history.interruption_requested(&accepted)? {
                state.lifecycle = state.lifecycle.interrupt()?;
                let terminal = self
                    .history
                    .append(&accepted, NewItem::Terminal(TerminalOutcome::Interrupted))?;
                state.published.push(terminal);
                reached_terminal = true;
                break;
            }
        }
        drop(stream);

        if !reached_terminal {
            state.lifecycle = state.lifecycle.fail()?;
            let terminal = self.history.append(
                &accepted,
                NewItem::Terminal(TerminalOutcome::Failed {
                    code: "PROVIDER_STREAM_ENDED".to_owned(),
                }),
            )?;
            state.published.push(terminal);
        }
        self.finish(
            &command.trust.tenant_id,
            &accepted,
            state.lifecycle,
            state.published,
        )
    }

    fn finish(
        &self,
        tenant_id: &TenantId,
        accepted: &AcceptedTurn,
        lifecycle: Turn,
        published: Vec<Item>,
    ) -> Result<TurnResult, TurnRunError> {
        Ok(TurnResult {
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
            status: lifecycle.status(),
            published,
            replay: self.history.replay(tenant_id, accepted.turn_id)?,
        })
    }
}

fn handle_event<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    event: ProviderEvent,
) -> Result<bool, TurnRunError> {
    match event {
        ProviderEvent::Delta(content) => {
            let durable = history.append(accepted, NewItem::AgentMessageDelta { content })?;
            state.published.push(durable);
            Ok(false)
        }
        ProviderEvent::Usage(observed) => {
            state.usage = observed;
            history.append(accepted, NewItem::Usage(observed))?;
            Ok(false)
        }
        ProviderEvent::Completed => {
            state.lifecycle = state.lifecycle.complete()?;
            let terminal = history.append(
                accepted,
                NewItem::Terminal(TerminalOutcome::Completed { usage: state.usage }),
            )?;
            state.published.push(terminal);
            Ok(true)
        }
        ProviderEvent::Error { code } => {
            state.lifecycle = state.lifecycle.fail()?;
            let terminal = history.append(
                accepted,
                NewItem::Terminal(TerminalOutcome::Failed { code }),
            )?;
            state.published.push(terminal);
            Ok(true)
        }
    }
}
