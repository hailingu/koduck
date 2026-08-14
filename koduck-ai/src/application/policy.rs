// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Default-deny policy for owned Tool and MCP actions.

use crate::domain::execution::{ApprovalRequirement, ExactActionBinding};
use crate::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};

/// C-5 approval scope that a C-7-validated principal must carry to resolve a
/// requested D-6 (ADR-0003 TC-05); defined by the domain so both the scope
/// authorizer and the sealed approver capability share one constant.
pub use crate::domain::execution::TOOL_APPROVAL_SCOPE;

/// A stable reason why C-5 denied an action before approval or dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DenialCode {
    /// No configured descriptor matched the action.
    DescriptorMissing,
    /// The configured descriptor exceeded its freshness window.
    DescriptorStale,
    /// The configured descriptor is disabled.
    DescriptorDisabled,
    /// The configured descriptor version is incompatible.
    DescriptorIncompatible,
    /// Descriptor metadata conflicts with another configured source or action.
    DescriptorConflicting,
    /// The effect is missing or unsupported.
    UnknownEffect,
    /// The immutable Turn profile does not contain the exact capability tuple.
    OutsidePermissionProfile,
    /// Action parameters do not satisfy the configured descriptor schema.
    InvalidInput,
}

/// The only three policy outcomes before canonical execution preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyDecision {
    /// The action cannot create an Approval Request or Execution Attempt.
    Denied(DenialCode),
    /// The exact read-only action may prepare an attempt without D-6.
    AllowWithoutApproval,
    /// The action requires one canonical exact-attempt D-6.
    RequireApproval,
}

/// Stateless C-5 policy evaluator over validated, configured values.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolPolicy;

/// Trusted configuration port that owns descriptor and Permission Profile snapshots.
///
/// Implementations belong to runtime configuration adapters. Request, model, Tool,
/// and MCP content must never implement or replace this dependency at runtime.
pub(crate) trait ToolPolicyConfiguration {
    /// Returns the configured descriptor snapshot for this exact action identity.
    fn descriptor_for(&self, action: &Action) -> Option<&CapabilityDescriptor>;

    /// Returns the immutable Turn profile snapshot for this exact identity and version.
    fn profile_for(&self, profile_id: &str, profile_version: &str) -> Option<&PermissionProfile>;
}

/// An immutable value snapshot of the configured descriptors and Permission
/// Profiles that may authorize C-5 bindings.
///
/// Every entry is an already validated owned domain value, so constructing a
/// snapshot cannot forge policy authority; runtime assembly decides which
/// snapshots exist in production, and the initial production snapshot is empty.
#[derive(Clone, Debug, Default)]
pub struct ToolConfigurationSnapshot {
    descriptors: Vec<CapabilityDescriptor>,
    profiles: Vec<PermissionProfile>,
}

/// A rejected snapshot registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolConfigurationError {
    /// A descriptor with the same identifier and version is already registered.
    DuplicateDescriptor,
    /// A profile with the same identifier and version is already registered.
    DuplicateProfile,
}

impl ToolConfigurationSnapshot {
    /// Creates an empty snapshot; this is the only production-authorized shape
    /// until a later accepted capability record enables descriptors.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            descriptors: Vec::new(),
            profiles: Vec::new(),
        }
    }

    /// Registers one validated descriptor snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ToolConfigurationError::DuplicateDescriptor`] without mutation
    /// when the exact descriptor identity and version is already registered.
    pub fn register_descriptor(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<(), ToolConfigurationError> {
        let duplicate = self.descriptors.iter().any(|existing| {
            existing.id() == descriptor.id() && existing.version() == descriptor.version()
        });
        if duplicate {
            return Err(ToolConfigurationError::DuplicateDescriptor);
        }
        self.descriptors.push(descriptor);
        Ok(())
    }

    /// Registers one validated Permission Profile snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ToolConfigurationError::DuplicateProfile`] without mutation
    /// when the exact profile identity and version is already registered.
    pub fn register_profile(
        &mut self,
        profile: PermissionProfile,
    ) -> Result<(), ToolConfigurationError> {
        let duplicate = self.profiles.iter().any(|existing| {
            existing.id() == profile.id() && existing.version() == profile.version()
        });
        if duplicate {
            return Err(ToolConfigurationError::DuplicateProfile);
        }
        self.profiles.push(profile);
        Ok(())
    }
}

impl ToolPolicyConfiguration for ToolConfigurationSnapshot {
    fn descriptor_for(&self, action: &Action) -> Option<&CapabilityDescriptor> {
        // An unregistered identity stays None so default-deny policy rejects
        // the action (TC-02).
        self.descriptors.iter().find(|existing| {
            existing.id() == action.descriptor_id()
                && existing.version() == action.descriptor_version()
        })
    }

    fn profile_for(&self, profile_id: &str, profile_version: &str) -> Option<&PermissionProfile> {
        self.profiles
            .iter()
            .find(|existing| existing.id() == profile_id && existing.version() == profile_version)
    }
}

/// C-5 authorization boundary backed by one injected trusted configuration source.
pub(crate) struct ToolAuthorizationService<C> {
    configuration: C,
}

impl<C> ToolAuthorizationService<C>
where
    C: ToolPolicyConfiguration,
{
    /// Creates the sole binding-sealing service around a trusted configuration adapter.
    #[must_use]
    pub(crate) const fn new(configuration: C) -> Self {
        Self { configuration }
    }

    /// Resolves configured values, evaluates policy, and seals one exact binding.
    ///
    /// # Errors
    ///
    /// Returns the exact default-deny reason without creating D-6 or D-7 authority.
    pub(crate) fn authorize_binding(
        &self,
        mut binding: ExactActionBinding,
    ) -> Result<ExactActionBinding, DenialCode> {
        let descriptor = self.configuration.descriptor_for(binding.action());
        let profile = self
            .configuration
            .profile_for(binding.profile_id(), binding.profile_version())
            .ok_or(DenialCode::OutsidePermissionProfile)?;
        if profile.id() != binding.profile_id() || profile.version() != binding.profile_version() {
            return Err(DenialCode::OutsidePermissionProfile);
        }
        let decision = ToolPolicy.evaluate(descriptor, binding.action(), profile);
        let requirement = match decision {
            PolicyDecision::Denied(reason) => return Err(reason),
            PolicyDecision::AllowWithoutApproval => ApprovalRequirement::NotRequired,
            PolicyDecision::RequireApproval => ApprovalRequirement::Required,
        };
        binding.authorize_policy(requirement);
        Ok(binding)
    }
}

impl ToolPolicy {
    /// Evaluates one action without mutating its immutable Permission Profile.
    #[must_use]
    pub fn evaluate(
        self,
        descriptor: Option<&CapabilityDescriptor>,
        action: &Action,
        profile: &PermissionProfile,
    ) -> PolicyDecision {
        let Some(descriptor) = descriptor else {
            return PolicyDecision::Denied(DenialCode::DescriptorMissing);
        };
        let state_denial = match descriptor.state() {
            DescriptorState::Active => None,
            DescriptorState::Stale => Some(DenialCode::DescriptorStale),
            DescriptorState::Disabled => Some(DenialCode::DescriptorDisabled),
            DescriptorState::Incompatible => Some(DenialCode::DescriptorIncompatible),
            DescriptorState::Conflicting => Some(DenialCode::DescriptorConflicting),
        };
        if let Some(reason) = state_denial {
            return PolicyDecision::Denied(reason);
        }
        if descriptor.effect() == Effect::Unknown || action.effect() == Effect::Unknown {
            return PolicyDecision::Denied(DenialCode::UnknownEffect);
        }
        if descriptor.id() != action.descriptor_id()
            || descriptor.version() != action.descriptor_version()
            || descriptor.effect() != action.effect()
        {
            return PolicyDecision::Denied(DenialCode::DescriptorConflicting);
        }
        if !descriptor.accepts_parameters(action.parameter_value()) {
            return PolicyDecision::Denied(DenialCode::InvalidInput);
        }
        if !profile.allows(
            descriptor.id(),
            descriptor.version(),
            descriptor.effect(),
            action.target(),
        ) {
            return PolicyDecision::Denied(DenialCode::OutsidePermissionProfile);
        }
        if descriptor.effect() == Effect::ReadData {
            PolicyDecision::AllowWithoutApproval
        } else {
            PolicyDecision::RequireApproval
        }
    }
}
