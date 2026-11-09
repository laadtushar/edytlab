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
    /// `<message> at <instance path>`, joined with `; ` (jsonschema 0.52).
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

    /// What [`schema_for`] derives, byte for byte, as schemars 0.8.22 wrote
    /// it: an `input_schema` is what every provider is sent, and some
    /// refuse shapes they do not know, so a schemars upgrade must not
    /// change it. The type covers the shapes a tool's arguments use:
    /// integers of each width (with their `format` and an unsigned
    /// `minimum`), floats, options, lists, a fixed-size array, a tuple, a
    /// map, a nested struct (a `$ref`), plain and documented enums, a
    /// default, a rename and doc comments of several lines and paragraphs.
    ///
    /// There are no `definitions`: 0.8's `schema_for!` returned them beside
    /// the schema, so this function has never written them, and a nested
    /// type's `$ref` does not resolve. No tool calls `schema_for` (each
    /// writes its `input_schema` by hand), so that is kept as it was.
    #[test]
    fn schema_for_writes_what_schemars_0_8_wrote() {
        use std::collections::BTreeMap;

        /// A nested part.
        #[derive(serde::Deserialize, JsonSchema)]
        #[allow(dead_code)]
        struct Point {
            x: f32,
            y: f32,
        }

        #[derive(serde::Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        enum Plain {
            First,
            SecondOne,
        }

        /// How a change is made.
        #[derive(serde::Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        enum Documented {
            /// Once only.
            Once,
            /// Over and over.
            Repeat,
        }

        /// Arguments of a tool, as a tool would derive them.
        #[derive(serde::Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Args {
            /// The track, from 0.
            track: u32,
            /// Samples from the start
            /// of the track.
            offset: i64,
            gain: f32,
            ratio: f64,
            enabled: bool,
            label: String,
            /// Optional gain in dB.
            gain_db: Option<f64>,
            limit: Option<u16>,
            name: Option<String>,
            tracks: Vec<usize>,
            pan: [f32; 2],
            levels: BTreeMap<String, f32>,
            /// Where it lands.
            at: Point,
            plain: Plain,
            mode: Documented,
            maybe_mode: Option<Documented>,
            #[serde(default)]
            loud: bool,
            #[serde(rename = "type")]
            kind: String,
            small: i8,
            byte: u8,
            count: Option<u64>,
            pair: (f32, String),
            ///
            ///   Indented, with a blank line before.
            ///
            /// A second paragraph.
            notes: Vec<Option<i16>>,
        }

        let got = serde_json::to_string(&schema_for::<Args>()).unwrap();
        assert_eq!(
            got, PINNED_SCHEMA_FOR_ARGS,
            "schema_for changed what it writes"
        );
    }

    /// Measured with schemars 0.8.22 (`serde_json` sorts the keys).
    const PINNED_SCHEMA_FOR_ARGS: &str = r##"{"additionalProperties":false,"description":"Arguments of a tool, as a tool would derive them.","properties":{"at":{"allOf":[{"$ref":"#/definitions/Point"}],"description":"Where it lands."},"byte":{"format":"uint8","minimum":0.0,"type":"integer"},"count":{"format":"uint64","minimum":0.0,"type":["integer","null"]},"enabled":{"type":"boolean"},"gain":{"format":"float","type":"number"},"gain_db":{"description":"Optional gain in dB.","format":"double","type":["number","null"]},"label":{"type":"string"},"levels":{"additionalProperties":{"format":"float","type":"number"},"type":"object"},"limit":{"format":"uint16","minimum":0.0,"type":["integer","null"]},"loud":{"default":false,"type":"boolean"},"maybe_mode":{"anyOf":[{"$ref":"#/definitions/Documented"},{"type":"null"}]},"mode":{"$ref":"#/definitions/Documented"},"name":{"type":["string","null"]},"notes":{"description":"Indented, with a blank line before.\n\nA second paragraph.","items":{"format":"int16","type":["integer","null"]},"type":"array"},"offset":{"description":"Samples from the start of the track.","format":"int64","type":"integer"},"pair":{"items":[{"format":"float","type":"number"},{"type":"string"}],"maxItems":2,"minItems":2,"type":"array"},"pan":{"items":{"format":"float","type":"number"},"maxItems":2,"minItems":2,"type":"array"},"plain":{"$ref":"#/definitions/Plain"},"ratio":{"format":"double","type":"number"},"small":{"format":"int8","type":"integer"},"track":{"description":"The track, from 0.","format":"uint32","minimum":0.0,"type":"integer"},"tracks":{"items":{"format":"uint","minimum":0.0,"type":"integer"},"type":"array"},"type":{"type":"string"}},"required":["at","byte","enabled","gain","label","levels","mode","notes","offset","pair","pan","plain","ratio","small","track","tracks","type"],"type":"object"}"##;

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
