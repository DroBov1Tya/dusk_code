//! Argument schemas. Built-in tools derive theirs from the type they parse, so the schema
//! the model reads and the parser cannot drift apart. Every call is checked against the
//! schema before the tool runs; a wrong call comes back to the model as a format error
//! it can fix in the same turn.

use jsonschema::Validator;
use schemars::{JsonSchema, generate::SchemaSettings};
use serde_json::Value;

/// Problems listed per call; the first few are enough to fix a call.
const MAX_PROBLEMS: usize = 5;
/// The schema is quoted back only while it is short; the tool list has it anyway.
const MAX_QUOTED_SCHEMA: usize = 2048;

/// The arguments schema of `T`, as providers accept it: one object at the top, every
/// subschema inline, no meta keys. An absent optional field is just not required.
pub fn of<T: JsonSchema>() -> Value {
    let settings = SchemaSettings::draft2020_12().with(|settings| {
        settings.inline_subschemas = true;
        settings.meta_schema = None;
    });
    let mut schema = settings.into_generator().root_schema_for::<T>().to_value();
    drop_null(&mut schema);
    if let Some(object) = schema.as_object_mut() {
        object.remove("title");
        // Doc comments on the type are for this code, not for the model.
        object.remove("description");
    }
    schema
}

/// `Option` fields allow null; models only need to leave them out, and some providers
/// reject a list of types.
fn drop_null(schema: &mut Value) {
    match schema {
        Value::Object(object) => {
            if let Some(Value::Array(types)) = object.get_mut("type") {
                types.retain(|kind| kind != "null");
                if let [only] = types.as_slice() {
                    let only = only.clone();
                    object.insert("type".into(), only);
                }
            }
            if let Some(Value::Array(choices)) = object.get("anyOf") {
                let kept: Vec<Value> = choices
                    .iter()
                    .filter(|choice| choice.get("type").is_none_or(|kind| kind != "null"))
                    .cloned()
                    .collect();
                if let [only] = kept.as_slice() {
                    let only = only.clone();
                    object.remove("anyOf");
                    if let Value::Object(only) = only {
                        object.extend(only);
                    }
                }
            }
            object.values_mut().for_each(drop_null);
        }
        Value::Array(items) => items.iter_mut().for_each(drop_null),
        _ => {}
    }
}

/// A compiled schema. A server's schema that does not compile checks nothing: the server
/// still validates its own input.
pub struct Check(Option<Validator>);

impl Check {
    pub fn new(schema: &Value) -> Self {
        match Validator::new(schema) {
            Ok(validator) => Self(Some(validator)),
            Err(error) => {
                tracing::warn!("tool schema does not compile, calls are not checked: {error}");
                Self(None)
            }
        }
    }

    /// What is wrong with `arguments`, written so the model can correct the call.
    /// Values are masked: they may be long or quote files.
    pub fn problems(&self, tool: &str, schema: &Value, arguments: &Value) -> Option<String> {
        let validator = self.0.as_ref()?;
        let problems: Vec<String> = validator
            .iter_errors(arguments)
            .take(MAX_PROBLEMS)
            .map(|error| {
                let at = match error.instance_path().as_str() {
                    "" => "arguments".to_owned(),
                    path => path.to_owned(),
                };
                format!("- {at}: {}", error.masked())
            })
            .collect();
        if problems.is_empty() {
            return None;
        }
        let mut text = format!(
            "invalid arguments for {tool}, nothing was done:\n{}\n",
            problems.join("\n")
        );
        let schema = schema.to_string();
        if schema.len() <= MAX_QUOTED_SCHEMA {
            text.push_str(&format!("The arguments must match: {schema}\n"));
        }
        text.push_str(&format!("Fix the arguments and call {tool} again."));
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Example {
        /// The file.
        path: String,
        limit: Option<u32>,
        #[serde(default)]
        items: Vec<String>,
    }

    #[test]
    fn a_wrong_call_says_where_and_what() {
        let schema = of::<Example>();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["path"]));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["limit"]["type"], "integer");
        let check = Check::new(&schema);
        let good = json!({ "path": "a", "items": ["x"] });
        assert!(check.problems("t", &schema, &good).is_none());
        let wrong = json!({ "path": "a", "items": [{ "text": "x" }], "extra": 1 });
        let problems = check.problems("t", &schema, &wrong).unwrap();
        assert!(problems.contains("/items/0"), "{problems}");
        assert!(problems.contains("extra"), "{problems}");
        assert!(problems.ends_with("call t again."), "{problems}");
    }
}
