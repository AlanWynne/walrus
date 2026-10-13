//! Shape detection and the deterministic `*_at` JSON walk for the Automerge
//! adapter (a `#[path]` child of `source/automerge.rs`, kept separate so each
//! file stays under the 400 non-test-line standard, NFR-1).
//!
//! This file is part of the ONE `automerge`-aware module, so it names
//! `automerge` types freely; it is reached only under `#[cfg(feature =
//! "automerge")]` via its parent.

use anyhow::{Context, Result};
use automerge::{Automerge, ChangeHash, ObjId, ObjType, ReadDoc, ScalarValue, Value, ROOT};

// === Shape ==============================================================

/// How a loaded document is presented, detected once at open and fixed for the
/// source's lifetime so renderer selection is stable across playback.
pub(crate) enum Shape {
    /// The root map has exactly one key whose value is a Text object:
    /// reconstruct yields that text's bytes. `mime`/`name` drive the renderer.
    Text {
        key: String,
        name: String,
        mime: String,
    },
    /// Anything else (zero or many keys, nested maps/lists, scalars):
    /// reconstruct serialises the whole tree to deterministic pretty JSON.
    Json { name: String },
}

impl Shape {
    /// Detect the shape of `doc` at its CURRENT heads.
    ///
    /// Text iff the root map has EXACTLY one key and that key's value is a Text
    /// object; Json otherwise.
    pub(crate) fn detect(doc: &Automerge, name: &str) -> Shape {
        let keys: Vec<String> = doc.keys(ROOT).collect();
        if keys.len() == 1 {
            let key = &keys[0];
            if let Ok(Some((Value::Object(ObjType::Text), _id))) = doc.get(ROOT, key) {
                let mime = mime_for_key(key);
                return Shape::Text {
                    key: key.clone(),
                    name: name.to_string(),
                    mime,
                };
            }
        }
        Shape::Json {
            name: name.to_string(),
        }
    }

    /// The `(mime, name)` hint used to pick a renderer/exporter.
    pub(crate) fn content_hint(&self) -> (String, String) {
        match self {
            Shape::Text { name, mime, .. } => (mime.clone(), name.clone()),
            Shape::Json { name } => ("application/json".to_string(), name.clone()),
        }
    }

    /// The structured-view status-bar note: `Some(..)` for Json, `None` for
    /// Text.
    pub(crate) fn status_note(&self) -> Option<String> {
        match self {
            Shape::Text { .. } => None,
            Shape::Json { .. } => Some("Automerge (structured JSON view)".to_string()),
        }
    }

    /// Reconstruct the document bytes at `heads`.
    ///
    /// # Errors
    /// Returns an error if an `automerge` read or the JSON serialisation fails.
    /// A Text document whose key is absent at `heads` yields EMPTY bytes (a
    /// valid "no content yet" state), not an error.
    pub(crate) fn reconstruct(&self, doc: &Automerge, heads: &[ChangeHash]) -> Result<Vec<u8>> {
        match self {
            Shape::Text { key, .. } => match doc.get_at(ROOT, key, heads) {
                Ok(Some((Value::Object(ObjType::Text), id))) => {
                    let text = doc
                        .text_at(&id, heads)
                        .context("reading Automerge text at heads")?;
                    Ok(text.into_bytes())
                }
                // Key absent (or not a Text) at these heads: no content yet.
                _ => Ok(Vec::new()),
            },
            Shape::Json { .. } => {
                let value = walk_map(doc, &ROOT, heads)?;
                serde_json::to_vec_pretty(&value).context("serialising Automerge tree to JSON")
            }
        }
    }
}

/// The mime for a single-text-root key: refine from the key's extension, else
/// `text/plain`. The renderer selector refines further from the name.
fn mime_for_key(key: &str) -> String {
    match key.rsplit('.').next() {
        Some("md") | Some("markdown") => "text/markdown",
        Some("html") | Some("htm") => "text/html",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        _ => "text/plain",
    }
    .to_string()
}

// === the deterministic *_at JSON walk ===================================

/// Recursively map the map object `obj` at `heads` to a `serde_json::Value`.
///
/// Keys are emitted in SORTED order so output is byte-identical for identical
/// documents regardless of automerge's internal iteration order.
fn walk_map(doc: &Automerge, obj: &ObjId, heads: &[ChangeHash]) -> Result<serde_json::Value> {
    let mut keys: Vec<String> = doc.keys_at(obj, heads).collect();
    keys.sort();
    let mut map = serde_json::Map::with_capacity(keys.len());
    for key in keys {
        if let Some((value, id)) = doc.get_at(obj, &key, heads).context("reading map key")? {
            map.insert(key, walk_value(doc, value, &id, heads)?);
        }
    }
    Ok(serde_json::Value::Object(map))
}

/// Map the list object `obj` at `heads` to a JSON array.
fn walk_list(doc: &Automerge, obj: &ObjId, heads: &[ChangeHash]) -> Result<serde_json::Value> {
    let len = doc.length_at(obj, heads);
    let mut items = Vec::with_capacity(len);
    for index in 0..len {
        if let Some((value, id)) = doc
            .get_at(obj, index, heads)
            .context("reading list index")?
        {
            items.push(walk_value(doc, value, &id, heads)?);
        }
    }
    Ok(serde_json::Value::Array(items))
}

/// Map a single automerge value (scalar or object) at `heads` to JSON.
fn walk_value(
    doc: &Automerge,
    value: Value<'_>,
    id: &ObjId,
    heads: &[ChangeHash],
) -> Result<serde_json::Value> {
    match value {
        Value::Object(ObjType::Map) | Value::Object(ObjType::Table) => walk_map(doc, id, heads),
        Value::Object(ObjType::List) => walk_list(doc, id, heads),
        Value::Object(ObjType::Text) => {
            let text = doc.text_at(id, heads).context("reading nested text")?;
            Ok(serde_json::Value::String(text))
        }
        Value::Scalar(scalar) => Ok(scalar_to_json(scalar.as_ref())),
    }
}

/// Map an automerge scalar to JSON.
///
/// `Bytes` becomes an array of `u8` numbers (lossless, renderer-safe). An
/// `Unknown` scalar is rare and not round-trippable, so it maps to `Null`.
fn scalar_to_json(scalar: &ScalarValue) -> serde_json::Value {
    use serde_json::Value as J;
    match scalar {
        ScalarValue::Str(s) => J::String(s.to_string()),
        ScalarValue::Int(i) => J::Number((*i).into()),
        ScalarValue::Uint(u) => J::Number((*u).into()),
        ScalarValue::F64(f) => serde_json::Number::from_f64(*f)
            .map(J::Number)
            .unwrap_or(J::Null),
        ScalarValue::Counter(c) => J::Number(i64::from(c).into()),
        ScalarValue::Timestamp(t) => J::Number((*t).into()),
        ScalarValue::Boolean(b) => J::Bool(*b),
        ScalarValue::Bytes(bytes) => {
            J::Array(bytes.iter().map(|b| J::Number((*b).into())).collect())
        }
        ScalarValue::Null => J::Null,
        // An unknown scalar type is not round-trippable to JSON; map to Null.
        ScalarValue::Unknown { .. } => J::Null,
    }
}
