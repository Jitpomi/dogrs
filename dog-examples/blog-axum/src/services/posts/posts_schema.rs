use anyhow::Result;
use dog_core::errors::DogError;
use dog_core::schema::HookMeta;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use validator::Validate;

use crate::services::RelayParams;

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
struct CreatePostData {
    #[validate(length(min = 1))]
    title: String,
    #[validate(length(min = 1))]
    body: String,
    #[serde(default)]
    published: bool,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
struct PatchPostData {
    #[validate(length(min = 1))]
    title: Option<String>,
    #[validate(length(min = 1))]
    body: Option<String>,
    published: Option<bool>,
}

#[derive(Clone, Copy)]
struct PostsLimits {
    title_min_len: usize,
    body_min_len: usize,
    body_max_len: usize,
}

fn limits(meta: &HookMeta<Value, RelayParams>) -> PostsLimits {
    PostsLimits {
        title_min_len: meta.config.get_usize("posts.titleMinLen").unwrap_or(1),
        body_min_len: meta.config.get_usize("posts.bodyMinLen").unwrap_or(0),
        body_max_len: meta.config.get_usize("posts.bodyMaxLen").unwrap_or(10_000),
    }
}

fn unprocessable(message: &str, errors: serde_json::Value) -> anyhow::Error {
    DogError::unprocessable(message)
        .with_errors(errors)
        .into_anyhow()
}

fn validator_errors_to_json(errs: &validator::ValidationErrors) -> serde_json::Value {
    // Shape: { field: ["msg1", "msg2"], nested: {..} }
    let mut out = serde_json::Map::new();

    for (field, kind) in errs.errors() {
        match kind {
            validator::ValidationErrorsKind::Field(field_errors) => {
                let msgs: Vec<String> = field_errors
                    .iter()
                    .map(|e| {
                        e.message
                            .as_ref()
                            .map(|m| m.to_string())
                            .unwrap_or_else(|| e.code.to_string())
                    })
                    .collect();
                out.insert(field.to_string(), json!(msgs));
            }
            validator::ValidationErrorsKind::Struct(struct_errs) => {
                out.insert(field.to_string(), validator_errors_to_json(struct_errs.as_ref()));
            }
            validator::ValidationErrorsKind::List(list_errs) => {
                // best-effort: return list of nested errors
                let mut list = serde_json::Map::new();
                for (idx, nested) in list_errs {
                    list.insert(idx.to_string(), validator_errors_to_json(nested.as_ref()));
                }
                out.insert(field.to_string(), serde_json::Value::Object(list));
            }
        }
    }

    serde_json::Value::Object(out)
}

fn validate_or_unprocessable<T: Validate>(value: &T) -> Result<()> {
    value
        .validate()
        .map_err(|e| unprocessable("Posts schema validation failed", validator_errors_to_json(&e)))
}

fn parse_validated<T>(data: &Value) -> Result<T>
where
    T: DeserializeOwned + Validate,
{
    let parsed: T = serde_json::from_value(data.clone()).map_err(|e| {
        unprocessable(
            "Posts schema validation failed",
            json!({"_schema": [e.to_string()]}),
        )
    })?;
    validate_or_unprocessable(&parsed)?;
    Ok(parsed)
}

fn resolve_all(data: &mut Value, _meta: &HookMeta<Value, RelayParams>) -> Result<()> {
    let Some(obj) = data.as_object_mut() else {
        return Ok(());
    };

    if let Some(Value::String(s)) = obj.get_mut("title") {
        *s = s.trim().to_string();
    }
    if let Some(Value::String(s)) = obj.get_mut("body") {
        *s = s.trim().to_string();
    }

    Ok(())
}

fn enforce_limits(
    title: Option<&str>,
    body: Option<&str>,
    lim: PostsLimits,
) -> Result<()> {
    if let Some(title) = title {
        if title.chars().count() < lim.title_min_len {
            return Err(unprocessable(
                "Posts schema validation failed",
                json!({"title": [format!("must be at least {} chars", lim.title_min_len)]}),
            ));
        }
    }

    if let Some(body) = body {
        if body.chars().count() < lim.body_min_len {
            return Err(unprocessable(
                "Posts schema validation failed",
                json!({"body": [format!("must be at least {} chars", lim.body_min_len)]}),
            ));
        }
        if body.chars().count() > lim.body_max_len {
            return Err(unprocessable(
                "Posts schema validation failed",
                json!({"body": [format!("must be at most {} chars", lim.body_max_len)]}),
            ));
        }
    }

    Ok(())
}

pub fn resolve_create(data: &mut Value, meta: &HookMeta<Value, RelayParams>) -> Result<()> {
    resolve_all(data, meta)?;

    let Some(obj) = data.as_object_mut() else {
        return Ok(());
    };

    if !obj.contains_key("published") {
        obj.insert("published".to_string(), Value::Bool(false));
    }

    Ok(())
}

pub fn validate_create(data: &Value, meta: &HookMeta<Value, RelayParams>) -> Result<()> {
    let parsed: CreatePostData = parse_validated(data)?;
    let _ = parsed.published;
    enforce_limits(
        Some(parsed.title.as_str()),
        Some(parsed.body.as_str()),
        limits(meta),
    )
}

pub fn validate_patch(data: &Value, meta: &HookMeta<Value, RelayParams>) -> Result<()> {
    let parsed: PatchPostData = parse_validated(data)?;
    let _ = parsed.published;
    enforce_limits(parsed.title.as_deref(), parsed.body.as_deref(), limits(meta))
}
