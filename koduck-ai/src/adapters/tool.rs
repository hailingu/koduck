// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Fail-closed JSON and JSON Schema translation into protocol-neutral Tool values.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use thiserror::Error;

use crate::domain::tool::{
    ActionParameters, InputSchema, JsonNumber, JsonValue, JsonValueKind, MAX_ACTION_INPUT_BYTES,
    ToolValueError,
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
    serde_json::from_str::<UniqueJson>(serialized).map_err(|_| ToolAdapterError::InvalidJson)?;
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
    serde_json::from_str::<UniqueJson>(serialized).map_err(|_| ToolAdapterError::InvalidSchema)?;
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

struct UniqueJson;

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = UniqueJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object members")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        let _ = value;
        Ok(UniqueJson)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        let _ = value;
        Ok(UniqueJson)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson)
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        let _ = value;
        Ok(UniqueJson)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        let _ = value;
        Ok(UniqueJson)
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        let _ = value;
        Ok(UniqueJson)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<UniqueJson>()?.is_some() {}
        Ok(UniqueJson)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut names = std::collections::BTreeSet::new();
        while let Some(name) = map.next_key::<String>()? {
            if !names.insert(name) {
                return Err(de::Error::custom("duplicate JSON object member"));
            }
            map.next_value::<UniqueJson>()?;
        }
        Ok(UniqueJson)
    }
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
