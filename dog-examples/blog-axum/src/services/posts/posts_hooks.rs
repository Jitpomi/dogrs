// Feathers-style hooks live in crate::services::setup for now.

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use dog_core::errors::DogError;
use dog_core::hooks::{DogAfterHook, DogBeforeHook, HookContext, HookResult};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::services::BlogParams;

use super::PostParams;

fn now_ts() -> String {
    Utc::now().to_rfc3339()
}

pub struct ValidatePostAuthorExists;

#[async_trait]
impl DogBeforeHook<Value, BlogParams> for ValidatePostAuthorExists {
    async fn run(&self, ctx: &mut HookContext<Value, BlogParams>) -> Result<()> {
        let Some(data) = ctx.data.as_ref() else {
            return Ok(());
        };

        let Some(obj) = data.as_object() else {
            return Ok(());
        };

        let Some(author_id) = obj.get("author_id") else {
            return Ok(());
        };

        if author_id.is_null() {
            return Ok(());
        }

        let Some(author_id) = author_id.as_str() else {
            return Err(DogError::unprocessable("Posts schema validation failed")
                .with_errors(json!({"author_id": ["must be a string"]}))
                .into_anyhow());
        };

        if author_id.trim().is_empty() {
            return Err(DogError::unprocessable("Posts schema validation failed")
                .with_errors(json!({"author_id": ["must not be empty"]}))
                .into_anyhow());
        }

        // Ensure the author exists in this tenant.
        let authors = ctx.services.service::<Value, BlogParams>("authors")?;
        let res = authors.get(&ctx.tenant, author_id, ctx.params.clone()).await;
        if res.is_err() {
            return Err(DogError::unprocessable("Posts schema validation failed")
                .with_errors(json!({"author_id": ["author not found"]}))
                .into_anyhow());
        }

        Ok(())
    }
}

fn should_expand_author(ctx: &HookContext<Value, BlogParams>) -> bool {
    if let Some(expand) = ctx.params.query.get("expand") {
        let expand = expand.trim();
        if expand.is_empty() {
            return false;
        }

        return expand
            .split(',')
            .map(|s| s.trim())
            .any(|s| s == "author");
    }

    ctx.config
        .get_bool("posts.expandAuthorDefault")
        .unwrap_or(false)
}

async fn expand_one_author(ctx: &HookContext<Value, BlogParams>, mut v: Value) -> Result<Value> {
    let Some(obj) = v.as_object_mut() else {
        return Ok(v);
    };

    let Some(author_id) = obj.get("author_id").and_then(|v| v.as_str()) else {
        return Ok(Value::Object(obj.clone()));
    };

    let authors = ctx.services.service::<Value, BlogParams>("authors")?;
    if let Ok(author) = authors.get(&ctx.tenant, author_id, ctx.params.clone()).await {
        obj.insert("author".to_string(), author);
    }

    Ok(Value::Object(obj.clone()))
}

pub struct ExpandPostAuthor;

#[async_trait]
impl DogAfterHook<Value, BlogParams> for ExpandPostAuthor {
    async fn run(&self, ctx: &mut HookContext<Value, BlogParams>) -> Result<()> {
        if !should_expand_author(ctx) {
            return Ok(());
        }

        let Some(res) = ctx.result.take() else {
            return Ok(());
        };

        ctx.result = Some(match res {
            HookResult::One(v) => HookResult::One(expand_one_author(ctx, v).await?),
            HookResult::Many(vs) => {
                let mut out = Vec::with_capacity(vs.len());
                for v in vs {
                    out.push(expand_one_author(ctx, v).await?);
                }
                HookResult::Many(out)
            }
        });

        Ok(())
    }
}

fn non_empty_string(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

fn bool_or(v: Option<&Value>, default: bool) -> bool {
    v.and_then(|v| v.as_bool()).unwrap_or(default)
}

fn normalize_one(v: Value, default_body: &str) -> Value {
    let obj = v.as_object().cloned().unwrap_or_default();

    let id = non_empty_string(obj.get("id"))
        .unwrap_or_else(|| format!("post:{}", Uuid::new_v4()));

    let title = non_empty_string(obj.get("title"))
        .unwrap_or_else(|| "Untitled".to_string());
    let body = non_empty_string(obj.get("body"))
        .unwrap_or_else(|| default_body.to_string());
    let published = bool_or(obj.get("published"), false);

    let author_id = non_empty_string(obj.get("author_id"));
    let author = obj.get("author").cloned();

    let ts = now_ts();
    let created_at = non_empty_string(obj.get("createdAt"))
        .unwrap_or_else(|| ts.clone());
    let updated_at = non_empty_string(obj.get("updatedAt"))
        .unwrap_or_else(|| ts.clone());

    let mut out = json!({
        "id": id,
        "title": title,
        "body": body,
        "published": published,
        "createdAt": created_at,
        "updatedAt": updated_at,
    });

    if let Some(author_id) = author_id {
        if let Some(map) = out.as_object_mut() {
            map.insert("author_id".to_string(), Value::String(author_id));
        }
    }

    if let Some(author) = author {
        if let Some(map) = out.as_object_mut() {
            map.insert("author".to_string(), author);
        }
    }

    out
}

pub struct NormalizePostsResult;

#[async_trait]
impl DogAfterHook<Value, BlogParams> for NormalizePostsResult {
    async fn run(&self, ctx: &mut HookContext<Value, BlogParams>) -> Result<()> {
        let post_params = PostParams::from(&ctx.params);
        let default_body = if post_params.include_drafts {
            ctx.config
                .get_string("posts.defaultBodyDrafts")
                .unwrap_or_else(|| "No body".to_string())
        } else {
            ctx.config
                .get_string("posts.defaultBody")
                .unwrap_or_else(|| "No body".to_string())
        };

        let Some(res) = ctx.result.take() else {
            return Ok(());
        };

        ctx.result = Some(match res {
            HookResult::One(v) => HookResult::One(normalize_one(v, &default_body)),
            HookResult::Many(vs) => HookResult::Many(
                vs.into_iter()
                    .map(|v| normalize_one(v, &default_body))
                    .collect(),
            ),
        });

        Ok(())
    }
}
