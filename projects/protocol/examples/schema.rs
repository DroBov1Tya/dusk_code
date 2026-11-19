//! Writes `docs/api/schema.json`, the published JSON Schema of the wire protocol, or with
//! `--check` verifies the file is current and that it only grew.
//!
//! ```text
//! cargo run -p dusk_protocol --features schema --example schema            # write
//! cargo run -p dusk_protocol --features schema --example schema -- --check # CI
//! ```
//!
//! While the file's `x-oldest` equals `OLDEST_PROTOCOL`, a removed property or variant, or a
//! property that became required, is reported as `not additive: <def>.<name>` and fails: an
//! older client still speaks to this core. Raising `OLDEST_PROTOCOL` starts a new baseline.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

use dusk_protocol::client::{
    Greeting, Hello, OLDEST_PROTOCOL, PROTOCOL_VERSION, Proof, RequestFrame, ServerFrame,
};
use schemars::SchemaGenerator;
use serde_json::{Map, Value};

/// The types a client meets first, listed first in the file.
const ROOTS: [&str; 5] = ["Hello", "Proof", "Greeting", "RequestFrame", "ServerFrame"];

fn main() -> ExitCode {
    let path = schema_path();
    let generated = generate();
    if std::env::args().any(|a| a == "--check") {
        return check(&path, &generated);
    }
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("cannot create {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    match std::fs::write(&path, &generated) {
        Ok(()) => {
            println!("wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cannot write {}: {e}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/schema.json")
}

/// The schema as text: handshake types first, then every other type by name.
fn generate() -> String {
    let mut generator = SchemaGenerator::default();
    generator.subschema_for::<Hello>();
    generator.subschema_for::<Proof>();
    generator.subschema_for::<Greeting>();
    generator.subschema_for::<RequestFrame>();
    generator.subschema_for::<ServerFrame>();
    let defs = generator.take_definitions(true);
    render(&defs)
}

fn render(defs: &Map<String, Value>) -> String {
    let mut names: Vec<&String> = defs.keys().collect();
    names.sort_by_key(|n| (ROOTS.iter().position(|r| r == n).unwrap_or(ROOTS.len()), *n));
    let mut text = String::from("{\n");
    text.push_str("  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n");
    text.push_str("  \"title\": \"duskcode protocol\",\n");
    text.push_str(&format!("  \"x-protocol\": {PROTOCOL_VERSION},\n"));
    text.push_str(&format!("  \"x-oldest\": {OLDEST_PROTOCOL},\n"));
    let roots = serde_json::to_string(&ROOTS).unwrap_or_default();
    text.push_str(&format!("  \"x-roots\": {roots},\n"));
    text.push_str("  \"$defs\": {\n");
    for (i, name) in names.iter().enumerate() {
        let body = serde_json::to_string_pretty(&defs[*name]).unwrap_or_default();
        let key = serde_json::to_string(name).unwrap_or_default();
        text.push_str(&format!("    {key}: {}", body.replace('\n', "\n    ")));
        text.push_str(if i + 1 < names.len() { ",\n" } else { "\n" });
    }
    text.push_str("  }\n}\n");
    text
}

fn check(path: &PathBuf, generated: &str) -> ExitCode {
    let Ok(on_disk) = std::fs::read_to_string(path) else {
        eprintln!(
            "{} is missing; run the example without --check",
            path.display()
        );
        return ExitCode::FAILURE;
    };
    if on_disk == generated {
        return ExitCode::SUCCESS;
    }
    let (old, new) = (
        serde_json::from_str::<Value>(&on_disk),
        serde_json::from_str::<Value>(generated),
    );
    if let (Ok(old), Ok(new)) = (old, new)
        && old["x-oldest"].as_u64() == Some(u64::from(OLDEST_PROTOCOL))
    {
        for problem in not_additive(&old, &new) {
            println!("not additive: {problem}");
        }
    }
    eprintln!(
        "docs/api/schema.json is out of date; run: cargo run -p dusk_protocol --features schema --example schema"
    );
    ExitCode::FAILURE
}

/// What the old schema had and the new one lacks, or demands where it did not.
fn not_additive(old: &Value, new: &Value) -> Vec<String> {
    let (old_defs, new_defs) = (&old["$defs"], &new["$defs"]);
    let Some(old_defs) = old_defs.as_object() else {
        return Vec::new();
    };
    let mut problems = Vec::new();
    for (name, old_def) in old_defs {
        let Some(new_def) = new_defs.get(name) else {
            problems.push(name.clone());
            continue;
        };
        let (before, after) = (Shape::of(name, old_def), Shape::of(name, new_def));
        problems.extend(before.names.difference(&after.names).cloned());
        problems.extend(after.required.difference(&before.required).cloned());
    }
    problems
}

/// The names a schema defines, as `Def.name` paths, and which of them are required.
#[derive(Default)]
struct Shape {
    /// Properties and variants.
    names: BTreeSet<String>,
    required: BTreeSet<String>,
}

impl Shape {
    fn of(def: &str, schema: &Value) -> Shape {
        let mut shape = Shape::default();
        shape.walk(def, schema, false);
        shape
    }

    fn walk(&mut self, path: &str, schema: &Value, variant: bool) {
        let Some(object) = schema.as_object() else {
            return;
        };
        for value in object
            .get("enum")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(variant) = value.as_str() {
                self.names.insert(format!("{path}.{variant}"));
            }
        }
        if let Some(variant) = object.get("const").and_then(Value::as_str) {
            self.names.insert(format!("{path}.{variant}"));
        }
        let required: Vec<&str> = object
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            // An externally tagged variant is an object whose one required property is
            // its name; it is a variant, not a field that can become required.
            let wrapper = variant && properties.len() == 1 && required.len() == 1;
            for (name, value) in properties {
                let at = format!("{path}.{name}");
                self.names.insert(at.clone());
                if !wrapper && required.contains(&name.as_str()) {
                    self.required.insert(at.clone());
                }
                self.walk(&at, value, false);
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            for part in object
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.walk(path, part, key != "allOf");
            }
        }
        if let Some(items) = object.get("items") {
            self.walk(path, items, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(def: Value) -> Value {
        json!({ "x-oldest": OLDEST_PROTOCOL, "$defs": { "Thing": def } })
    }

    #[test]
    fn removing_a_variant_is_not_additive() {
        let old = schema(json!({ "oneOf": [
            { "type": "string", "enum": ["Alpha", "Beta"] },
            { "type": "object", "properties": { "Gamma": { "type": "integer" } },
              "required": ["Gamma"], "additionalProperties": false }
        ]}));
        let new = schema(json!({ "oneOf": [
            { "type": "string", "enum": ["Alpha"] }
        ]}));
        let mut problems = not_additive(&old, &new);
        problems.sort();
        assert_eq!(problems, ["Thing.Beta", "Thing.Gamma"]);
    }

    #[test]
    fn removing_a_property_or_requiring_one_is_not_additive() {
        let old = schema(json!({ "properties": { "a": {}, "b": {} }, "required": ["a"] }));
        let new = schema(json!({ "properties": { "a": {}, "c": {} }, "required": ["a", "c"] }));
        assert_eq!(not_additive(&old, &new), ["Thing.b", "Thing.c"]);
        let removed = schema(json!({ "properties": { "a": {} } }));
        assert_eq!(not_additive(&old, &removed), ["Thing.b"]);
    }

    #[test]
    fn adding_an_optional_field_is_additive() {
        let old = schema(json!({ "properties": { "a": {} }, "required": ["a"] }));
        let new = schema(json!({
            "properties": { "a": {}, "b": { "type": "string" } },
            "required": ["a"]
        }));
        assert!(not_additive(&old, &new).is_empty());
    }

    #[test]
    fn adding_a_variant_or_a_type_is_additive() {
        let old = schema(json!({ "oneOf": [{ "type": "string", "enum": ["Alpha"] }] }));
        let mut new = schema(json!({ "oneOf": [
            { "type": "string", "enum": ["Alpha", "Beta"] },
            { "type": "object", "properties": { "Gamma": {} },
              "required": ["Gamma"], "additionalProperties": false }
        ]}));
        new["$defs"]["Other"] = json!({ "type": "string" });
        assert!(not_additive(&old, &new).is_empty());
    }
}
