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

use jsonschema::Validator;
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
    // Pin Draft 7: jsonschema's default became 2020-12 in 0.25, and the
    // schemas schemars derives (and the hand-written ones) assume Draft 7.
    let compiled = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .build(input_schema)
        .map_err(|e| format!("invalid input_schema: {e}"))?;

    if compiled.is_valid(args) {
        Ok(())
    } else {
        Err(join_errors(&compiled, args))
    }
}

/// Every validation error of `args`, as `<message> at <instance path>`
/// joined with `; ` (the shape the model is shown).
pub(crate) fn join_errors(compiled: &Validator, args: &Value) -> String {
    compiled
        .iter_errors(args)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect::<Vec<_>>()
        .join("; ")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The text the model is shown on a rejected call: every error as
    /// `<message> at <instance path>`, joined with `; ` (jsonschema 0.60).
    #[test]
    fn errors_are_joined_with_their_instance_path() {
        let schema = json!({
            "type": "object",
            "properties": { "track": {}, "gain": { "type": "number" } },
            "required": ["track"],
            "additionalProperties": false,
        });
        let err = validate(&schema, &json!({ "gain": "loud" })).unwrap_err();
        eprintln!("{err}");
        assert!(err.contains("\"track\" is a required property"), "{err}");
        assert!(
            err.contains("\"loud\" is not of type \"number\" at /gain"),
            "{err}"
        );
        assert!(err.contains("; "), "{err}");
        assert!(validate(&schema, &json!({ "track": 1, "gain": 2.0 })).is_ok());
    }

    /// Draft 7 is pinned (0.25 made 2020-12 the default): `definitions`
    /// and a `$ref` into them, which schemars emits, must still resolve.
    #[test]
    fn draft_7_definitions_still_resolve() {
        let schema = json!({
            "type": "object",
            "properties": { "kind": { "$ref": "#/definitions/Kind" } },
            "definitions": { "Kind": { "type": "string", "enum": ["a", "b"] } },
        });
        assert!(validate(&schema, &json!({ "kind": "a" })).is_ok());
        assert!(validate(&schema, &json!({ "kind": "c" })).is_err());
    }
}
