// Feathers-style hooks live in crate::services::setup for now.

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use dog_core::hooks::{DogAfterHook, HookContext, HookResult};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::services::BlogParams;

use super::PostParams;

fn now_ts() -> String {
    Utc::now().to_rfc3339()
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

    let ts = now_ts();
    let created_at = non_empty_string(obj.get("createdAt"))
        .unwrap_or_else(|| ts.clone());
    let updated_at = non_empty_string(obj.get("updatedAt"))
        .unwrap_or_else(|| ts.clone());

    json!({
        "id": id,
        "title": title,
        "body": body,
        "published": published,
        "createdAt": created_at,
        "updatedAt": updated_at,
    })
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
