use super::super::error::{FeatureKind, UnsupportedFeatures};
use serde_json::Value;

pub(super) fn chat_tools_to_responses(
    value: &Value,
    pointer: &str,
) -> Result<Vec<Value>, UnsupportedFeatures> {
    let tools = value.as_array().ok_or_else(|| {
        UnsupportedFeatures::single(
            FeatureKind::UnsupportedField,
            pointer,
            "tools must be an array",
        )
    })?;
    tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            let p = format!("{pointer}/{i}");
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::BuiltinTool,
                    format!("{p}/type"),
                    "only function tools are supported",
                ));
            }
            if let Some(object) = tool.as_object() {
                for key in object.keys() {
                    if !["type", "function"].contains(&key.as_str()) {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::UnsupportedField,
                            format!("{p}/{key}"),
                            "tool property is not representable",
                        ));
                    }
                }
            }
            let function = tool.get("function").ok_or_else(|| {
                UnsupportedFeatures::single(
                    FeatureKind::MissingToolField,
                    format!("{p}/function"),
                    "function tool requires function object",
                )
            })?;
            let function = function.as_object().ok_or_else(|| {
                UnsupportedFeatures::single(
                    FeatureKind::MissingToolField,
                    format!("{p}/function"),
                    "function must be an object",
                )
            })?;
            for key in function.keys() {
                if !["name", "description", "parameters", "strict"].contains(&key.as_str()) {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::UnsupportedField,
                        format!("{p}/function/{key}"),
                        "tool property is not representable",
                    ));
                }
            }
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    UnsupportedFeatures::single(
                        FeatureKind::MissingToolField,
                        format!("{p}/function/name"),
                        "function tool requires name",
                    )
                })?;
            let parameters = function
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"type":"object", "properties":{}}));
            if !parameters.is_object() {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::InvalidToolArguments,
                    format!("{p}/function/parameters"),
                    "parameters must be an object",
                ));
            }
            let mut result =
                serde_json::json!({"type":"function", "name":name, "parameters":parameters});
            if let Some(description) = function.get("description") {
                result["description"] = description.clone();
            }
            if let Some(strict) = function.get("strict") {
                result["strict"] = strict.clone();
            }
            Ok(result)
        })
        .collect()
}

pub(super) fn chat_tool_choice_to_responses(
    value: &Value,
    pointer: &str,
) -> Result<Option<Value>, UnsupportedFeatures> {
    match value {
        Value::String(value) if matches!(value.as_str(), "auto" | "none" | "required") => {
            Ok(Some(Value::String(value.to_string())))
        }
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("function") => {
            let name = object
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    UnsupportedFeatures::single(
                        FeatureKind::MissingToolField,
                        format!("{pointer}/function/name"),
                        "function tool_choice requires name",
                    )
                })?;
            if object.keys().any(|key| key != "type" && key != "function") {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnsupportedField,
                    pointer,
                    "tool_choice contains an unrepresentable field",
                ));
            }
            Ok(Some(serde_json::json!({"type":"function", "name":name})))
        }
        _ => Err(UnsupportedFeatures::single(
            FeatureKind::UnsupportedField,
            pointer,
            "unsupported Chat tool_choice",
        )),
    }
}
