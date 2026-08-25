// ADR: koduck-ai/docs/adr/ADR-0001-strict-json-duplicate-member-validation.md

//! Recursive duplicate-member rejection for one bounded untrusted JSON document.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};

/// Rejects duplicate JSON object members anywhere in one bounded document.
///
/// `serde_json::Value` silently keeps the last duplicate member, so untrusted
/// input is first rejected when any object member is duplicated at any nesting
/// depth — two `finish_reason` values must never collapse into one validated
/// finish (ADR-0004 PSC-3/PSC-5), and duplicate Tool parameters or descriptor
/// schema members must never collapse into one accepted value (ADR-0003).
/// Callers retain ownership of byte bounds, check ordering, and typed error
/// mapping (ADR-0001 SJ-03/SJ-04).
///
/// # Errors
///
/// Returns the underlying [`serde_json::Error`] for malformed JSON or any
/// duplicated object member name.
pub(crate) fn ensure_unique_members(serialized: &str) -> Result<(), serde_json::Error> {
    serde_json::from_str::<UniqueJson>(serialized).map(|_| ())
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

#[cfg(test)]
mod tests {
    use super::ensure_unique_members;

    #[test]
    fn rejects_malformed_and_duplicate_json_at_every_depth() {
        let valid = [
            "null",
            "true",
            "false",
            "-9223372036854775808",
            "-1",
            "0",
            "42",
            "18446744073709551615",
            "-0.5",
            "3.25",
            "\"\"",
            "\"text\"",
            "[]",
            "[1,\"two\",null,[3.5,[true]]]",
            "{}",
            "{\"a\":1}",
            "{\"a\":{\"b\":{\"c\":[{\"d\":null}]}}}",
        ];
        for input in valid {
            assert!(
                ensure_unique_members(input).is_ok(),
                "valid duplicate-free JSON was rejected: {input}"
            );
        }

        let malformed = [
            "",
            "nul",
            "tru",
            "\"unterminated",
            "{\"a\":",
            "{\"a\" 1}",
            "{1:2}",
            "[1,2",
            "[,]",
            "{}}",
        ];
        for input in malformed {
            assert!(
                ensure_unique_members(input).is_err(),
                "malformed JSON was accepted: {input}"
            );
        }

        let duplicates = [
            "{\"a\":1,\"a\":2}",
            "{\"a\":{\"b\":1,\"b\":2}}",
            "{\"a\":{\"b\":{\"c\":1,\"c\":2}}}",
            "{\"a\":{\"b\":{\"c\":{\"d\":1,\"d\":2}}}}",
            "[{\"a\":1,\"a\":2}]",
            "{\"a\":[{\"b\":1,\"b\":2}]}",
        ];
        for input in duplicates {
            assert!(
                ensure_unique_members(input).is_err(),
                "duplicate members were accepted: {input}"
            );
        }
    }
}
