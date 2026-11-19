//! Writes `docs/api/schema.json`, the published JSON Schema of the wire protocol, or with
//! `--check` verifies the file is current and that it only grew.
//!
//! ```text
//! cargo run -p dusk_protocol --features schema --example schema            # write
//! cargo run -p dusk_protocol --features schema --example schema -- --check # CI
//! cargo run -p dusk_protocol --features schema --example schema -- --check --base main.json
//! ```
//!
//! `--base <file>` compares the generated schema with another one, such as `main`'s copy of
//! the file, apart from the "file is current" check: regenerating the file in a branch then
//! cannot hide a change that is not additive.
//!
//! While the file's `x-oldest` equals `OLDEST_PROTOCOL`, a removed property or variant, or a
//! property that became required, is reported as `not additive: <def>.<name>` and fails: an
//! older client still speaks to this core. A property whose `type` or `$ref` changed fails
//! too. Not caught: internally tagged variants that share field names (their fields merge
//! under one path), and enum values inside a referenced type, which are checked only through
//! that type's own entry. A branch of a union is a variant named by its constant tag
//! property (`Widget.Badge.label`) or by its single wrapper property.
//!
//! Nullability: `T` to `Option<T>` is not additive, since an old reader with a plain field
//! rejects `null`. Both shapes, `type: [t, "null"]` and `anyOf [{$ref}, {type: null}]`, are
//! read as `nullable <t>`, and a change between `t` and `nullable t` is `changed type`.
//! Raising `OLDEST_PROTOCOL` starts a new baseline.

use std::collections::{BTreeMap, BTreeSet};
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
    let generated = match generate() {
        Ok(text) => text,
        Err(e) => {
            eprintln!("cannot render the schema: {e}");
            return ExitCode::FAILURE;
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(at) = args.iter().position(|a| a == "--base") {
        let Some(base) = args.get(at + 1) else {
            eprintln!("--base needs a file");
            return ExitCode::FAILURE;
        };
        let verdict = check_base(&PathBuf::from(base), &generated);
        // A base check alone never writes the file.
        if verdict == ExitCode::FAILURE || !args.iter().any(|a| a == "--check") {
            return verdict;
        }
    }
    if args.iter().any(|a| a == "--check") {
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
fn generate() -> serde_json::Result<String> {
    let mut generator = SchemaGenerator::default();
    generator.subschema_for::<Hello>();
    generator.subschema_for::<Proof>();
    generator.subschema_for::<Greeting>();
    generator.subschema_for::<RequestFrame>();
    generator.subschema_for::<ServerFrame>();
    let defs = generator.take_definitions(true);
    render(&defs)
}

fn render(defs: &Map<String, Value>) -> serde_json::Result<String> {
    let mut names: Vec<&String> = defs.keys().collect();
    names.sort_by_key(|n| (ROOTS.iter().position(|r| r == n).unwrap_or(ROOTS.len()), *n));
    let mut text = String::from("{\n");
    text.push_str("  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n");
    text.push_str("  \"title\": \"duskcode protocol\",\n");
    text.push_str(&format!("  \"x-protocol\": {PROTOCOL_VERSION},\n"));
    text.push_str(&format!("  \"x-oldest\": {OLDEST_PROTOCOL},\n"));
    let roots = serde_json::to_string(&ROOTS)?;
    text.push_str(&format!("  \"x-roots\": {roots},\n"));
    text.push_str("  \"$defs\": {\n");
    for (i, name) in names.iter().enumerate() {
        let body = serde_json::to_string_pretty(&defs[*name])?;
        let key = serde_json::to_string(name)?;
        text.push_str(&format!("    {key}: {}", body.replace('\n', "\n    ")));
        text.push_str(if i + 1 < names.len() { ",\n" } else { "\n" });
    }
    text.push_str("  }\n}\n");
    Ok(text)
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

/// Fails when the generated schema is not additive over `base`.
fn check_base(base: &PathBuf, generated: &str) -> ExitCode {
    let text = match std::fs::read_to_string(base) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("cannot read {}: {e}", base.display());
            return ExitCode::FAILURE;
        }
    };
    match base_problems(&text, generated) {
        Ok(problems) if problems.is_empty() => ExitCode::SUCCESS,
        Ok(problems) => {
            for problem in problems {
                println!("not additive: {problem}");
            }
            eprintln!("the schema is not additive over {}", base.display());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("cannot read {} as a schema: {e}", base.display());
            ExitCode::FAILURE
        }
    }
}

/// What the generated schema breaks of `base`; nothing when `base` belongs to an older
/// baseline (its `x-oldest` is not `OLDEST_PROTOCOL`).
fn base_problems(base: &str, generated: &str) -> serde_json::Result<Vec<String>> {
    let base: Value = serde_json::from_str(base)?;
    let generated: Value = serde_json::from_str(generated)?;
    if base["x-oldest"].as_u64() != Some(u64::from(OLDEST_PROTOCOL)) {
        return Ok(Vec::new());
    }
    Ok(not_additive(&base, &generated))
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
        // A new variant's own required fields are fine; a field that became required on
        // something that already existed is not.
        for at in after.required.difference(&before.required) {
            let parent = at.rsplit_once('.').map_or("", |(parent, _)| parent);
            let existed =
                before.names.contains(at) || !parent.contains('.') || before.names.contains(parent);
            if existed {
                problems.push(at.clone());
            }
        }
        for (at, was) in &before.kinds {
            if after.kinds.get(at).is_some_and(|now| now != was) {
                problems.push(format!("{at} changed type"));
            }
        }
    }
    problems
}

/// What a property is, for the type guard: its `$ref` or `type`, with `nullable ` in front
/// when it may be null, whether written `type: [t, "null"]` or `anyOf [{$ref}, {null}]`.
fn kind_of(value: &Value) -> Option<String> {
    if let Some(r) = value.get("$ref") {
        return Some(r.to_string());
    }
    match value.get("type") {
        Some(Value::Array(types)) => {
            let mut names = types.iter().filter_map(Value::as_str);
            let real: Vec<&str> = names.by_ref().filter(|t| *t != "null").collect();
            let nullable = types.iter().any(|t| t == "null");
            return Some(format!(
                "{}{}",
                if nullable { "nullable " } else { "" },
                real.join("|")
            ));
        }
        Some(t) => return Some(t.as_str().map_or_else(|| t.to_string(), str::to_owned)),
        None => {}
    }
    let parts = value.get("anyOf")?.as_array()?;
    let null = |p: &Value| p.get("type").and_then(Value::as_str) == Some("null");
    if parts.len() == 2 && parts.iter().any(null) {
        let other = parts.iter().find(|p| !null(p))?;
        return Some(format!("nullable {}", kind_of(other)?));
    }
    None
}

/// The names a schema defines, as `Def.name` paths, and which of them are required.
#[derive(Default)]
struct Shape {
    /// Properties and variants.
    names: BTreeSet<String>,
    required: BTreeSet<String>,
    /// The `type` or `$ref` of each property, where it has one.
    kinds: BTreeMap<String, String>,
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
            // A branch of a union names its variant by a property holding a constant (an
            // internally tagged enum: its fields live under that variant), or is an object
            // whose one required property is the variant's name (externally tagged).
            let tag = variant
                .then(|| {
                    properties
                        .iter()
                        .find_map(|(k, v)| v.get("const").and_then(Value::as_str).map(|c| (k, c)))
                })
                .flatten();
            let wrapper = variant && tag.is_none() && properties.len() == 1 && required.len() == 1;
            let base = match tag {
                Some((_, name)) => {
                    let at = format!("{path}.{name}");
                    self.names.insert(at.clone());
                    at
                }
                None => path.to_owned(),
            };
            for (name, value) in properties {
                if tag.is_some_and(|(k, _)| k == name) {
                    continue;
                }
                let at = format!("{base}.{name}");
                self.names.insert(at.clone());
                if let Some(kind) = kind_of(value) {
                    self.kinds.insert(at.clone(), kind);
                }
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
    fn a_base_catches_what_regenerating_the_file_hides() {
        let base = schema(json!({ "properties": { "a": {}, "b": {} } }));
        let generated = schema(json!({ "properties": { "a": {} } }));
        let problems = base_problems(&base.to_string(), &generated.to_string());
        assert_eq!(problems.ok(), Some(vec!["Thing.b".to_owned()]));
        assert_eq!(
            base_problems(&generated.to_string(), &base.to_string()).ok(),
            Some(Vec::new())
        );
        let mut older = base.clone();
        older["x-oldest"] = json!(OLDEST_PROTOCOL - 1);
        let skipped = base_problems(&older.to_string(), &generated.to_string());
        assert_eq!(skipped.ok(), Some(Vec::new()));
        assert!(base_problems("not json", &generated.to_string()).is_err());
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

    #[test]
    fn a_new_variant_may_carry_required_fields() {
        let old = schema(json!({ "oneOf": [{ "type": "string", "enum": ["Alpha"] }] }));
        let new = schema(json!({ "oneOf": [
            { "type": "string", "enum": ["Alpha"] },
            { "type": "object", "properties": { "Gamma": {
                "type": "object", "properties": { "x": { "type": "integer" } },
                "required": ["x"] } },
              "required": ["Gamma"], "additionalProperties": false }
        ]}));
        assert!(not_additive(&old, &new).is_empty());
    }

    #[test]
    fn changing_a_property_type_is_not_additive() {
        let old = schema(json!({ "properties": { "a": { "type": "string" } } }));
        let new = schema(json!({ "properties": { "a": { "type": "integer" } } }));
        assert_eq!(not_additive(&old, &new), ["Thing.a changed type"]);
    }

    fn tagged(branches: Vec<Value>) -> Value {
        schema(json!({ "oneOf": branches }))
    }

    fn branch(tag: &str, fields: &[(&str, bool)]) -> Value {
        let mut properties = Map::new();
        properties.insert("kind".into(), json!({ "const": tag, "type": "string" }));
        let mut required = vec![json!("kind")];
        for (name, needed) in fields {
            properties.insert((*name).into(), json!({ "type": "string" }));
            if *needed {
                required.push(json!(name));
            }
        }
        json!({ "type": "object", "properties": properties, "required": required })
    }

    #[test]
    fn a_new_tagged_variant_may_carry_required_fields() {
        let old = tagged(vec![branch("alpha", &[("text", true)])]);
        let new = tagged(vec![
            branch("alpha", &[("text", true)]),
            branch("badge", &[("label", true)]),
        ]);
        assert!(not_additive(&old, &new).is_empty());
    }

    #[test]
    fn an_existing_tagged_variant_gaining_a_required_field_is_not_additive() {
        let old = tagged(vec![
            branch("alpha", &[("text", true)]),
            branch("beta", &[]),
        ]);
        let new = tagged(vec![
            branch("alpha", &[("text", true), ("more", true)]),
            branch("beta", &[]),
        ]);
        assert_eq!(not_additive(&old, &new), ["Thing.alpha.more"]);
        let dropped = tagged(vec![branch("alpha", &[]), branch("beta", &[])]);
        assert_eq!(not_additive(&old, &dropped), ["Thing.alpha.text"]);
    }

    #[test]
    fn making_a_primitive_optional_is_not_additive() {
        let old = schema(json!({ "properties": { "a": { "type": "string" } } }));
        let new = schema(json!({ "properties": { "a": { "type": ["string", "null"] } } }));
        assert_eq!(not_additive(&old, &new), ["Thing.a changed type"]);
    }

    #[test]
    fn making_a_reference_optional_is_not_additive() {
        let plain = json!({ "$ref": "#/$defs/Other" });
        let optional = json!({ "anyOf": [{ "$ref": "#/$defs/Other" }, { "type": "null" }] });
        let old = schema(json!({ "properties": { "a": plain } }));
        let new = schema(json!({ "properties": { "a": optional } }));
        assert_eq!(not_additive(&old, &new), ["Thing.a changed type"]);
        assert!(not_additive(&new, &new).is_empty());
    }
}
