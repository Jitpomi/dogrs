use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::DogError;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::json;
use serde_json::Value;

use crate::services::BlogParams;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnDeletePolicy {
    Restrict,
    Cascade,
    Nullify,
}

fn parse_on_delete(s: &str) -> Option<OnDeletePolicy> {
    match s.trim().to_lowercase().as_str() {
        "restrict" => Some(OnDeletePolicy::Restrict),
        "cascade" => Some(OnDeletePolicy::Cascade),
        "nullify" => Some(OnDeletePolicy::Nullify),
        _ => None,
    }
}

fn resolve_policy(ctx: &HookContext<Value, BlogParams>) -> Result<OnDeletePolicy> {
    if let Some(v) = ctx.params.query.get("onDelete") {
        if let Some(p) = parse_on_delete(v) {
            return Ok(p);
        }
        return Err(DogError::bad_request("Invalid onDelete policy")
            .with_errors(json!({"onDelete": ["must be one of: restrict, cascade, nullify"]}))
            .into_anyhow());
    }

    if let Some(v) = ctx.config.get("authors.onDelete") {
        if let Some(p) = parse_on_delete(v) {
            return Ok(p);
        }
    }

    Ok(OnDeletePolicy::Restrict)
}

pub struct EnforceAuthorOnDelete;

#[async_trait]
impl DogBeforeHook<Value, BlogParams> for EnforceAuthorOnDelete {
    async fn run(&self, ctx: &mut HookContext<Value, BlogParams>) -> Result<()> {
        // Only applies to authors.remove
        let Some(author_id) = ctx.params.path.split('/').last() else {
            return Ok(());
        };

        let policy = resolve_policy(ctx)?;

        let posts = ctx.services.service::<Value, BlogParams>("posts")?;

        // Fetch all posts (including drafts) and filter by author_id.
        let mut params = ctx.params.clone();
        params.query.insert("includeDrafts".to_string(), "true".to_string());
        let all_posts = posts.find(&ctx.tenant, params.clone()).await?;

        let referencing: Vec<Value> = all_posts
            .into_iter()
            .filter(|p| p.get("author_id").and_then(|v| v.as_str()) == Some(author_id))
            .collect();

        if referencing.is_empty() {
            return Ok(());
        }

        match policy {
            OnDeletePolicy::Restrict => Err(DogError::conflict("Cannot delete author while posts reference it")
                .with_errors(json!({"_schema": ["cannot delete author with existing posts"]}))
                .into_anyhow()),
            OnDeletePolicy::Cascade => {
                for p in referencing {
                    if let Some(id) = p.get("id").and_then(|v| v.as_str()) {
                        let _ = posts.remove(&ctx.tenant, Some(id), ctx.params.clone()).await?;
                    }
                }
                Ok(())
            }
            OnDeletePolicy::Nullify => {
                for p in referencing {
                    if let Some(id) = p.get("id").and_then(|v| v.as_str()) {
                        let patch = json!({"author_id": Value::Null});
                        let _ = posts.patch(&ctx.tenant, Some(id), patch, ctx.params.clone()).await?;
                    }
                }
                Ok(())
            }
        }
    }
}
