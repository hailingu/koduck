// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: docs/adr/ADR-0004-provider-stream-completion-normalization.md

//! Duplicate-member rejection for provider JSON frames, provider-local so the
//! change stays inside this boundary (ADR-0004); mirrors the C-5 JSON
//! adapter's duplicate-property rejection (ADR-0003).

use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};

/// Rejects any JSON document containing duplicate object members.
///
/// `serde_json::Value` silently keeps the last duplicate member, so callers
/// that must fail closed on conflicting duplicated evidence — such as a
/// provider frame carrying two `finish_reason` values (ADR-0004 PSC-3) —
/// validate through this guard before structural parsing.
///
/// # Errors
///
/// Returns the underlying deserialization error, including the
/// duplicate-member diagnostic, when the document is malformed or contains
/// duplicate object members.
pub(super) fn ensure_unique(serialized: &str) -> Result<(), serde_json::Error> {
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

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
