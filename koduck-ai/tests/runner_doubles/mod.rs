// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Shared doubles for the black-box runner integration harnesses.
//!
//! Every runner-level integration crate drives the Turn kernel through the
//! same scripted provider contract and asserts through the same projection
//! emit order, so those doubles live here once instead of being redefined
//! per test crate with drifting semantics.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use koduck_ai::application::{
    ModelInput, ModelProvider, ProviderError, ProviderEvent, ProviderStream, ToolProjection,
    ToolProjectionSink,
};

/// Provider stub that serves one scripted stream per request and records
/// every request input, so tests can prove the continuation request and its
/// carried committed results.
#[derive(Clone, Default)]
pub(crate) struct ScriptedProvider {
    scripts: Arc<Mutex<VecDeque<Vec<ProviderEvent>>>>,
    inputs: Arc<Mutex<Vec<ModelInput>>>,
}

impl ScriptedProvider {
    /// Builds the double that serves each scripted event stream in order.
    pub(crate) fn scripted(scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into())),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns every provider request input seen so far, in order.
    pub(crate) fn recorded_inputs(&self) -> Vec<ModelInput> {
        self.inputs.lock().expect("inputs lock").clone()
    }
}

impl ModelProvider for ScriptedProvider {
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        self.inputs.lock().expect("inputs lock").push(input);
        let events = self
            .scripts
            .lock()
            .expect("scripts lock")
            .pop_front()
            .expect("one scripted stream per provider request");
        Ok(Box::new(events.into_iter()))
    }
}

/// Appends and publishes one projection, mirroring the production emit order.
pub(crate) fn emit_projection(sink: &mut dyn ToolProjectionSink, projection: &ToolProjection) {
    sink.append(projection).expect("fixture projection appends");
    sink.publish(projection);
}
