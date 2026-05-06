//! Helpers for building and validating tool schemas.
//!
//! Each [`Tool`](crate::Tool) returns its full Anthropic descriptor from
//! `schema()`; this module provides:
//!
//! * [`anthropic_tool`] — wrap a name, description, and an `input_schema`
//!   value into the canonical Anthropic shape.
//! * [`schema_for`] — derive an `input_schema` from a `JsonSchema`
//!   type, with the `$schema` and `title` keys stripped (Anthropic
//!   ignores them and they only add noise to the wire format).
//! * [`validate`] — validate a JSON value against a tool's `input_schema`
//!   using `jsonschema`. Returns a single combined error string so
//!   dispatch errors stay flat.

use jsonschema::JSONSchema;
use schemars::JsonSchema;
use serde_json::{json, Map, Value};

/// Build the Anthropic-shaped tool descriptor:
/// `{ "name": ..., "description": ..., "input_schema": ... }`.
///
/// `input_schema` should already be a JSON Schema object describing the
/// tool's arguments. Use [`schema_for`] if you have a Rust type with
/// `#[derive(JsonSchema)]`.
pub fn anthropic_tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "input_schema": input_schema,
    })
}

/// Derive an `input_schema` JSON value from a Rust type that implements
/// [`JsonSchema`]. Strips top-level `$schema` and `title` to keep the
/// payload tight.
pub fn schema_for<T: JsonSchema>() -> Value {
    let root = schemars::schema_for!(T);
    let mut value =
        serde_json::to_value(root.schema).expect("schemars schema serialization is infallible");
    if let Value::Object(map) = &mut value {
        map.remove("$schema");
        map.remove("title");
    }
    value
}

/// Validate `args` against `input_schema`. Returns the combined error
/// messages joined with `; ` if validation fails.
pub fn validate(input_schema: &Value, args: &Value) -> std::result::Result<(), String> {
    // jsonschema 0.18 wants a Draft7-or-later schema; tool schemas are
    // simple object schemas so this is fine in practice.
    let compiled = JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(input_schema)
        .map_err(|e| format!("invalid input_schema: {e}"))?;

    if let Err(errors) = compiled.validate(args) {
        let joined = errors
            .map(|e| format!("{} at {}", e, e.instance_path))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(joined);
    }
    Ok(())
}

/// Convenience: build a minimal `input_schema` object from a list of
/// `(name, type, required)` entries. Used by tools whose argument shape
/// is too small to merit a dedicated `JsonSchema`-deriving struct.
///
/// `type_name` should be one of the JSON Schema primitives:
/// `"string"`, `"number"`, `"integer"`, `"boolean"`, `"object"`,
/// `"array"`.
pub fn object_schema(props: &[(&str, &str, bool)]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, ty, req) in props {
        properties.insert((*name).to_string(), json!({ "type": ty }));
        if *req {
            required.push(Value::String((*name).to_string()));
        }
    }
    json!({
        "type": "object",
        "properties": Value::Object(properties),
        "required": Value::Array(required),
        "additionalProperties": false,
    })
}
