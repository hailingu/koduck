// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: koduck-ai/docs/adr/ADR-0001-strict-json-duplicate-member-validation.md

//! Fail-closed JSON and JSON Schema translation into protocol-neutral Tool values.

use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

use crate::domain::tool::{
    Action, ActionParameters, Effect, InputSchema, JsonNumber, JsonValue, JsonValueKind,
    MAX_ACTION_INPUT_BYTES, ToolValueError,
};

const MAX_DESCRIPTOR_SCHEMA_BYTES: usize = 65_536;

/// A rejected untrusted Tool protocol value.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolAdapterError {
    /// Serialized content is not valid or supported JSON.
    #[error("tool JSON is invalid or unsupported")]
    InvalidJson,
    /// The descriptor schema is invalid or outside the supported fail-closed subset.
    #[error("descriptor JSON Schema is invalid or unsupported")]
    InvalidSchema,
    /// Serialized descriptor schema exceeds the adapter resource limit.
    #[error("descriptor JSON Schema exceeds its owned byte limit")]
    SchemaTooLarge,
    /// Canonical action input exceeds the owned byte limit.
    #[error("tool action input exceeds its owned byte limit")]
    InputTooLarge,
    /// The translated fields do not form one bounded owned action.
    #[error("tool call fields do not form a bounded owned action")]
    InvalidAction,
    /// An untrusted Tool or MCP declaration addresses a different configured
    /// capability than the one its adapter was configured for.
    #[error("tool or MCP declaration does not address the configured capability")]
    CapabilityMismatch,
}

/// The configured capability snapshot one adapter may address.
///
/// The effect and target are trusted configuration resolved from the C-5
/// descriptor snapshot, never untrusted wire content: a native Tool call or
/// an MCP server declaration cannot relabel the effect or target it addresses
/// (ADR-0003 TC-01/TC-03).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfiguredCapability<'a> {
    descriptor_id: &'a str,
    descriptor_version: &'a str,
    effect: Effect,
    target: &'a str,
}

impl<'a> ConfiguredCapability<'a> {
    /// Creates one configured capability snapshot.
    #[must_use]
    pub const fn new(
        descriptor_id: &'a str,
        descriptor_version: &'a str,
        effect: Effect,
        target: &'a str,
    ) -> Self {
        Self {
            descriptor_id,
            descriptor_version,
            effect,
            target,
        }
    }

    /// Returns the configured descriptor ID.
    #[must_use]
    pub const fn descriptor_id(&self) -> &'a str {
        self.descriptor_id
    }
}

/// Translates one untrusted native Tool call into the owned action.
///
/// The serialized parameters use the same fail-closed translation as every
/// other untrusted Tool value; the effect and target come only from the
/// configured capability snapshot.
///
/// # Errors
///
/// Returns [`ToolAdapterError`] for malformed, oversized, or unbounded input.
pub fn translate_native_tool_call(
    configured: &ConfiguredCapability<'_>,
    parameters: &str,
) -> Result<Action, ToolAdapterError> {
    let parameters = parse_action_parameters(parameters)?;
    Action::new(
        configured.descriptor_id,
        configured.descriptor_version,
        configured.effect,
        configured.target,
        parameters,
    )
    .map_err(|_| ToolAdapterError::InvalidAction)
}

/// Translates one untrusted MCP tool call into the same owned action.
///
/// The server-declared name, schema, and arguments are untrusted: the name
/// must address exactly the configured capability, and an MCP transport or
/// server declaration can never alter the configured effect or target, so the
/// translated value is byte-identical to the native Tool translation of the
/// same call (ADR-0003 TC-01/TC-11).
///
/// # Errors
///
/// Returns [`ToolAdapterError`] when the declared name addresses a different
/// capability or the arguments fail the owned translation.
pub fn translate_mcp_tool_call(
    configured: &ConfiguredCapability<'_>,
    server_declared_name: &str,
    arguments: &str,
) -> Result<Action, ToolAdapterError> {
    if server_declared_name != configured.descriptor_id {
        return Err(ToolAdapterError::CapabilityMismatch);
    }
    translate_native_tool_call(configured, arguments)
}

/// Parses untrusted JSON parameters into a structurally valid owned value.
///
/// # Errors
///
/// Returns [`ToolAdapterError`] for malformed JSON, unsupported numbers, or size overflow.
pub fn parse_action_parameters(serialized: &str) -> Result<ActionParameters, ToolAdapterError> {
    if serialized.len() > MAX_ACTION_INPUT_BYTES {
        return Err(ToolAdapterError::InputTooLarge);
    }
    super::strict_json::ensure_unique_members(serialized)
        .map_err(|_| ToolAdapterError::InvalidJson)?;
    let value: Value =
        serde_json::from_str(serialized).map_err(|_| ToolAdapterError::InvalidJson)?;
    let value = convert_value(value)?;
    ActionParameters::new(value).map_err(|error| match error {
        ToolValueError::InputTooLarge => ToolAdapterError::InputTooLarge,
        ToolValueError::Invalid { .. } => ToolAdapterError::InvalidJson,
    })
}

/// Parses the supported object-only JSON Schema subset used by C-5.
///
/// # Errors
///
/// Returns [`ToolAdapterError::InvalidSchema`] for malformed or unsupported schemas.
pub fn parse_input_schema(serialized: &str) -> Result<InputSchema, ToolAdapterError> {
    if serialized.len() > MAX_DESCRIPTOR_SCHEMA_BYTES {
        return Err(ToolAdapterError::SchemaTooLarge);
    }
    super::strict_json::ensure_unique_members(serialized)
        .map_err(|_| ToolAdapterError::InvalidSchema)?;
    let schema: Value =
        serde_json::from_str(serialized).map_err(|_| ToolAdapterError::InvalidSchema)?;
    let object = schema.as_object().ok_or(ToolAdapterError::InvalidSchema)?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "type" | "properties" | "required" | "additionalProperties"
        )
    }) {
        return Err(ToolAdapterError::InvalidSchema);
    }
    if object.get("type").and_then(Value::as_str) != Some("object") {
        return Err(ToolAdapterError::InvalidSchema);
    }
    let property_values = object
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(ToolAdapterError::InvalidSchema)?;
    let mut properties = Vec::with_capacity(property_values.len());
    for (name, definition) in property_values {
        let definition = definition
            .as_object()
            .filter(|definition| definition.len() == 1 && definition.contains_key("type"))
            .ok_or(ToolAdapterError::InvalidSchema)?;
        let kind = definition
            .get("type")
            .and_then(Value::as_str)
            .and_then(parse_kind)
            .ok_or(ToolAdapterError::InvalidSchema)?;
        properties.push((name.clone(), kind));
    }
    let required = object
        .get("required")
        .and_then(Value::as_array)
        .ok_or(ToolAdapterError::InvalidSchema)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or(ToolAdapterError::InvalidSchema)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let additional_properties = object
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .ok_or(ToolAdapterError::InvalidSchema)?;
    InputSchema::object(properties, required, additional_properties)
        .map_err(|_| ToolAdapterError::InvalidSchema)
}

fn parse_kind(value: &str) -> Option<JsonValueKind> {
    match value {
        "string" => Some(JsonValueKind::String),
        "number" => Some(JsonValueKind::Number),
        "integer" => Some(JsonValueKind::Integer),
        "boolean" => Some(JsonValueKind::Boolean),
        "object" => Some(JsonValueKind::Object),
        "array" => Some(JsonValueKind::Array),
        "null" => Some(JsonValueKind::Null),
        _ => None,
    }
}

fn convert_value(value: Value) -> Result<JsonValue, ToolAdapterError> {
    match value {
        Value::Null => Ok(JsonValue::Null),
        Value::Bool(value) => Ok(JsonValue::Boolean(value)),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(JsonValue::Integer(value))
            } else if let Some(value) = value.as_u64() {
                Ok(JsonValue::UnsignedInteger(value))
            } else {
                JsonNumber::new(value.to_string())
                    .map(JsonValue::Number)
                    .map_err(|_| ToolAdapterError::InvalidJson)
            }
        }
        Value::String(value) => Ok(JsonValue::String(value)),
        Value::Array(values) => values
            .into_iter()
            .map(convert_value)
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array),
        Value::Object(values) => values
            .into_iter()
            .map(|(name, value)| Ok((name, convert_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, ToolAdapterError>>()
            .map(JsonValue::Object),
    }
}
