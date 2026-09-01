// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Owned Tool and MCP values that carry no adapter authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use thiserror::Error;

/// Maximum byte size for the stable descriptor identifier carried by an owned action.
pub const MAX_DESCRIPTOR_ID_BYTES: usize = 128;
/// Maximum byte size for the stable descriptor version carried by an owned action.
pub const MAX_DESCRIPTOR_VERSION_BYTES: usize = 128;
/// Maximum byte size for one owned Tool action target identifier.
pub const MAX_ACTION_TARGET_BYTES: usize = 256;
/// Maximum serialized and canonical byte size for one owned Tool action input.
pub const MAX_ACTION_INPUT_BYTES: usize = 65_536;
/// Maximum byte size for a Permission Profile identifier.
pub const MAX_PROFILE_ID_BYTES: usize = 128;
/// Maximum byte size for a Permission Profile version.
pub const MAX_PROFILE_VERSION_BYTES: usize = 128;

/// A validation failure for an owned Tool or MCP value.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolValueError {
    /// A required value was empty or contained unsupported bytes.
    #[error("{field} is invalid")]
    Invalid { field: &'static str },
    /// Serialized action input crossed the CAND-2 limit.
    #[error("action input exceeds 65536 bytes")]
    InputTooLarge,
}

/// One supported JSON value kind in a validated descriptor input schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonValueKind {
    String,
    Number,
    Integer,
    Boolean,
    Object,
    Array,
    Null,
}

/// Validated object-schema subset used by the owned policy boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSchema {
    properties: BTreeMap<String, JsonValueKind>,
    required: BTreeSet<String>,
    additional_properties: bool,
}

impl InputSchema {
    /// Creates a validated object schema from adapter-parsed JSON Schema fields.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError`] for blank, duplicate, or unknown required fields.
    pub fn object(
        properties: impl IntoIterator<Item = (String, JsonValueKind)>,
        required: impl IntoIterator<Item = String>,
        additional_properties: bool,
    ) -> Result<Self, ToolValueError> {
        let mut property_set = BTreeMap::new();
        for (name, kind) in properties {
            if name.trim().is_empty() || property_set.insert(name, kind).is_some() {
                return Err(ToolValueError::Invalid {
                    field: "schema_property",
                });
            }
        }
        let mut required_set = BTreeSet::new();
        for name in required {
            if !property_set.contains_key(&name) || !required_set.insert(name) {
                return Err(ToolValueError::Invalid {
                    field: "schema_required",
                });
            }
        }
        Ok(Self {
            properties: property_set,
            required: required_set,
            additional_properties,
        })
    }

    fn accepts(&self, parameters: &ActionParameters) -> bool {
        let JsonValue::Object(input) = parameters.value() else {
            return false;
        };
        if !self.required.iter().all(|name| input.contains_key(name)) {
            return false;
        }
        input.iter().all(|(name, value)| {
            self.properties
                .get(name)
                .map_or(self.additional_properties, |kind| kind.accepts(value))
        })
    }
}

impl JsonValueKind {
    fn accepts(self, value: &JsonValue) -> bool {
        matches!(
            (self, value),
            (Self::String, JsonValue::String(_))
                | (
                    Self::Number,
                    JsonValue::Integer(_) | JsonValue::UnsignedInteger(_) | JsonValue::Number(_)
                )
                | (
                    Self::Integer,
                    JsonValue::Integer(_) | JsonValue::UnsignedInteger(_)
                )
                | (Self::Boolean, JsonValue::Boolean(_))
                | (Self::Object, JsonValue::Object(_))
                | (Self::Array, JsonValue::Array(_))
                | (Self::Null, JsonValue::Null)
        )
    }
}

/// Protocol-neutral JSON value admitted by the adapter boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum JsonValue {
    Null,
    Boolean(bool),
    Integer(i64),
    UnsignedInteger(u64),
    /// A finite, canonical JSON number that is not representable as `i64`.
    Number(JsonNumber),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

/// A validated finite number using the JSON number grammar.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct JsonNumber(String);

impl JsonNumber {
    /// Validates and owns one exact JSON number spelling.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError`] when the value is empty, non-finite, or does
    /// not match JSON's number grammar exactly.
    pub fn new(value: impl Into<String>) -> Result<Self, ToolValueError> {
        let value = value.into();
        if valid_json_number(&value) {
            Ok(Self(value))
        } else {
            Err(ToolValueError::Invalid {
                field: "json_number",
            })
        }
    }

    /// Returns the exact validated JSON number spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_json_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    if !scan_json_integer(bytes, &mut index) {
        return false;
    }
    if !scan_json_fraction(bytes, &mut index) {
        return false;
    }
    if !scan_json_exponent(bytes, &mut index) {
        return false;
    }
    index == bytes.len()
}

/// Scans the integer part: `0` alone or a digit run that does not start with
/// `0`. Returns whether at least one digit was present.
fn scan_json_integer(bytes: &[u8], index: &mut usize) -> bool {
    if *index == bytes.len() {
        return false;
    }
    if bytes[*index] == b'0' {
        *index += 1;
        // A leading zero may not be followed by another digit.
        return !bytes.get(*index).is_some_and(u8::is_ascii_digit);
    }
    if !bytes[*index].is_ascii_digit() {
        return false;
    }
    *index += 1;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    true
}

/// Scans the optional fraction part: `.` followed by at least one digit.
/// Returns whether the part is absent or complete.
fn scan_json_fraction(bytes: &[u8], index: &mut usize) -> bool {
    if bytes.get(*index) != Some(&b'.') {
        return true;
    }
    *index += 1;
    let fraction_start = *index;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    *index > fraction_start
}

/// Scans the optional exponent part: `e`/`E`, an optional sign, and at least
/// one digit. Returns whether the part is absent or complete.
fn scan_json_exponent(bytes: &[u8], index: &mut usize) -> bool {
    if !matches!(bytes.get(*index), Some(b'e' | b'E')) {
        return true;
    }
    *index += 1;
    if matches!(bytes.get(*index), Some(b'+' | b'-')) {
        *index += 1;
    }
    let exponent_start = *index;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    *index > exponent_start
}

/// Structurally valid, canonically serialized action parameters.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ActionParameters {
    value: JsonValue,
    canonical: String,
}

impl ActionParameters {
    /// Creates parameters from a parsed protocol-neutral JSON value.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError::InputTooLarge`] when canonical JSON exceeds the cap.
    pub fn new(value: JsonValue) -> Result<Self, ToolValueError> {
        let mut canonical = String::new();
        write_json_value(&value, &mut canonical);
        if canonical.len() > MAX_ACTION_INPUT_BYTES {
            return Err(ToolValueError::InputTooLarge);
        }
        Ok(Self { value, canonical })
    }

    /// Returns the parsed value used for descriptor-schema validation.
    #[must_use]
    pub const fn value(&self) -> &JsonValue {
        &self.value
    }

    /// Returns deterministic canonical JSON used by exact-action hashing.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

fn write_json_value(value: &JsonValue, output: &mut String) {
    match value {
        JsonValue::Null => output.push_str("null"),
        JsonValue::Boolean(value) => output.push_str(if *value { "true" } else { "false" }),
        JsonValue::Integer(value) => output.push_str(&value.to_string()),
        JsonValue::UnsignedInteger(value) => output.push_str(&value.to_string()),
        JsonValue::Number(value) => output.push_str(value.as_str()),
        JsonValue::String(value) => write_json_string(value, output),
        JsonValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_json_value(value, output);
            }
            output.push(']');
        }
        JsonValue::Object(values) => {
            output.push('{');
            for (index, (name, value)) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_json_string(name, output);
                output.push(':');
                write_json_value(value, output);
            }
            output.push('}');
        }
    }
}

fn write_json_string(value: &str, output: &mut String) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04x}", character as u32)
                    .expect("writing to a String cannot fail");
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

/// The configured external effect classification for one capability.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Effect {
    /// Reads bounded, non-secret data.
    ReadData,
    /// Changes state outside canonical AI history.
    ExternalWrite,
    /// Changes files through the isolated executor.
    FilesystemWrite,
    /// Starts or signals a process through the isolated executor.
    ProcessExecute,
    /// Contacts a destination beyond the executor endpoint.
    NetworkEgress,
    /// Uses a credential reference at the executor boundary.
    CredentialUse,
    /// Represents missing or unsupported effect metadata.
    Unknown,
}

/// The validation and availability state of a configured descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorState {
    /// The descriptor is available for policy evaluation.
    Active,
    /// The descriptor exceeded its configured freshness window.
    Stale,
    /// The descriptor is administratively disabled.
    Disabled,
    /// The descriptor version is not supported by this runtime.
    Incompatible,
    /// Descriptor sources disagree about its immutable metadata.
    Conflicting,
}

/// A validated, configured capability descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDescriptor {
    id: String,
    version: String,
    effect: Effect,
    state: DescriptorState,
    input_schema: InputSchema,
}

impl CapabilityDescriptor {
    /// Creates a descriptor after validating its stable identity.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError`] for an invalid ID or version.
    pub fn new(
        id: impl Into<String>,
        version: impl Into<String>,
        effect: Effect,
        state: DescriptorState,
        input_schema: InputSchema,
    ) -> Result<Self, ToolValueError> {
        let id = id.into();
        let version = version.into();
        validate_descriptor_id(&id)?;
        validate_descriptor_version(&version)?;
        Ok(Self {
            id,
            version,
            effect,
            state,
            input_schema,
        })
    }

    /// Returns a copy with the supplied availability state.
    #[must_use]
    pub fn with_state(mut self, state: DescriptorState) -> Self {
        self.state = state;
        self
    }

    /// Returns the stable descriptor ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the fixed descriptor version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the configured effect classification.
    #[must_use]
    pub const fn effect(&self) -> Effect {
        self.effect
    }

    /// Returns the descriptor availability state.
    #[must_use]
    pub const fn state(&self) -> DescriptorState {
        self.state
    }

    /// Reports whether serialized parameters satisfy the validated descriptor schema.
    #[must_use]
    pub fn accepts_parameters(&self, parameters: &ActionParameters) -> bool {
        self.input_schema.accepts(parameters)
    }
}

/// One validated Tool or MCP action proposed by the model boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Action {
    descriptor_id: String,
    descriptor_version: String,
    effect: Effect,
    target: String,
    parameters: ActionParameters,
}

impl Action {
    /// Creates a bounded action whose adapter-neutral fields can be hashed later.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError`] for invalid identity, target, or input size.
    pub fn new(
        descriptor_id: impl Into<String>,
        descriptor_version: impl Into<String>,
        effect: Effect,
        target: impl Into<String>,
        parameters: ActionParameters,
    ) -> Result<Self, ToolValueError> {
        let descriptor_id = descriptor_id.into();
        let descriptor_version = descriptor_version.into();
        let target = target.into();
        validate_descriptor_id(&descriptor_id)?;
        validate_descriptor_version(&descriptor_version)?;
        validate_action_target(&target)?;
        Ok(Self {
            descriptor_id,
            descriptor_version,
            effect,
            target,
            parameters,
        })
    }

    /// Returns the referenced descriptor ID.
    #[must_use]
    pub fn descriptor_id(&self) -> &str {
        &self.descriptor_id
    }

    /// Returns the referenced descriptor version.
    #[must_use]
    pub fn descriptor_version(&self) -> &str {
        &self.descriptor_version
    }

    /// Returns the requested effect.
    #[must_use]
    pub const fn effect(&self) -> Effect {
        self.effect
    }

    /// Returns the exact target.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the bounded serialized parameters.
    #[must_use]
    pub fn parameters(&self) -> &str {
        self.parameters.canonical()
    }

    /// Returns parsed parameters for descriptor-schema validation.
    #[must_use]
    pub const fn parameter_value(&self) -> &ActionParameters {
        &self.parameters
    }
}

/// One immutable Turn-scoped permission profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionProfile {
    id: String,
    version: String,
    allowed: BTreeSet<(String, String, Effect, String)>,
}

impl PermissionProfile {
    /// Creates an empty default-deny profile.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError`] when the version is invalid.
    pub fn empty(
        id: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, ToolValueError> {
        Ok(Self::builder(id, version)?.build())
    }

    /// Starts construction of an immutable profile.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError`] when the version is invalid.
    pub fn builder(
        id: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<PermissionProfileBuilder, ToolValueError> {
        let id = id.into();
        let version = version.into();
        validate_profile_identity(&id, &version)
            .map_err(|field| ToolValueError::Invalid { field })?;
        Ok(PermissionProfileBuilder {
            id,
            version,
            allowed: BTreeSet::new(),
        })
    }

    /// Returns the immutable profile identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the immutable profile version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the number of explicitly permitted descriptor/effect tuples.
    #[must_use]
    pub fn allowed_capability_count(&self) -> usize {
        self.allowed.len()
    }

    /// Reports whether the exact descriptor version and effect is permitted.
    #[must_use]
    pub fn allows(&self, descriptor_id: &str, version: &str, effect: Effect, target: &str) -> bool {
        self.allowed.contains(&(
            descriptor_id.to_owned(),
            version.to_owned(),
            effect,
            target.to_owned(),
        ))
    }

    /// Returns the first exact target this profile permits for the descriptor
    /// version and effect, when the capability is in-profile at all.
    #[must_use]
    pub fn allowed_target(
        &self,
        descriptor_id: &str,
        version: &str,
        effect: Effect,
    ) -> Option<String> {
        self.allowed
            .iter()
            .find(|(id, entry_version, entry_effect, _)| {
                id == descriptor_id && entry_version == version && *entry_effect == effect
            })
            .map(|(_, _, _, target)| target.clone())
    }
}

/// Builder for one immutable Permission Profile.
#[derive(Clone, Debug)]
pub struct PermissionProfileBuilder {
    id: String,
    version: String,
    allowed: BTreeSet<(String, String, Effect, String)>,
}

impl PermissionProfileBuilder {
    /// Adds one exact configured capability tuple bounded by the action contract.
    ///
    /// # Errors
    ///
    /// Returns [`ToolValueError`] when the descriptor ID, version, or target
    /// fails the same bound enforced by [`Action::new`], so a profile cannot
    /// carry an envelope field wider than a valid owned action.
    pub fn allow(
        mut self,
        descriptor_id: impl Into<String>,
        version: impl Into<String>,
        effect: Effect,
        target: impl Into<String>,
    ) -> Result<Self, ToolValueError> {
        let descriptor_id = descriptor_id.into();
        let version = version.into();
        let target = target.into();
        validate_descriptor_id(&descriptor_id)?;
        validate_descriptor_version(&version)?;
        validate_action_target(&target)?;
        self.allowed
            .insert((descriptor_id, version, effect, target));
        Ok(self)
    }

    /// Finishes the immutable profile.
    #[must_use]
    pub fn build(self) -> PermissionProfile {
        PermissionProfile {
            id: self.id,
            version: self.version,
            allowed: self.allowed,
        }
    }
}

pub(crate) fn validate_descriptor_id(value: &str) -> Result<(), ToolValueError> {
    if value.is_empty()
        || value.len() > MAX_DESCRIPTOR_ID_BYTES
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        Err(ToolValueError::Invalid {
            field: "descriptor_id",
        })
    } else {
        Ok(())
    }
}

pub(crate) fn validate_descriptor_version(value: &str) -> Result<(), ToolValueError> {
    if value.is_empty()
        || value.len() > MAX_DESCRIPTOR_VERSION_BYTES
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        Err(ToolValueError::Invalid {
            field: "descriptor_version",
        })
    } else {
        Ok(())
    }
}

pub(crate) fn validate_action_target(value: &str) -> Result<(), ToolValueError> {
    if value.trim().is_empty()
        || value.len() > MAX_ACTION_TARGET_BYTES
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        Err(ToolValueError::Invalid { field: "target" })
    } else {
        Ok(())
    }
}

/// Returns the first profile identity field that violates the shared envelope
/// bound, so the profile constructor and the exact-action binding apply one
/// identical byte and character limit before the value is hashed, cloned, or
/// retained in D-6/D-7 state.
pub(crate) fn validate_profile_identity(id: &str, version: &str) -> Result<(), &'static str> {
    if !is_within_profile_bound(id, MAX_PROFILE_ID_BYTES) {
        Err("profile_id")
    } else if !is_within_profile_bound(version, MAX_PROFILE_VERSION_BYTES) {
        Err("profile_version")
    } else {
        Ok(())
    }
}

fn is_within_profile_bound(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_bytes
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_control())
}
