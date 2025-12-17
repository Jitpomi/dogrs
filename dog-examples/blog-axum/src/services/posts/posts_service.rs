use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;

use crate::services::adapters::blog_adapter::{BlogAdapter, StoreKind};
use crate::services::{BlogParams, BlogState};

use super::posts_shared;
use super::PostParams;

pub struct PostsService {
    pub adapter: BlogAdapter,
}

#[async_trait]
impl DogService<Value, BlogParams> for PostsService {
    fn capabilities(&self) -> ServiceCapabilities {
        posts_shared::crud_capabilities()
    }

    async fn create(&self, ctx: &TenantContext, data: Value, params: BlogParams) -> Result<Value> {
        self.adapter._create(ctx, data, params).await
    }

    async fn find(&self, ctx: &TenantContext, params: BlogParams) -> Result<Vec<Value>> {
        let post_params = PostParams::from(&params);
        let all = self.adapter._find(ctx, params).await?;
        Ok(all
            .into_iter()
            .filter(|v| {
                post_params.include_drafts
                    || v.get("published").and_then(|v| v.as_bool()).unwrap_or(false)
            })
            .collect())
    }

    async fn get(&self, ctx: &TenantContext, id: &str, params: BlogParams) -> Result<Value> {
        self.adapter._get(ctx, id, params).await
    }

    async fn update(&self, ctx: &TenantContext, id: &str, data: Value, params: BlogParams) -> Result<Value> {
        self.adapter._update(ctx, id, data, params).await
    }

    async fn patch(&self, ctx: &TenantContext, id: Option<&str>, data: Value, params: BlogParams) -> Result<Value> {
        self.adapter._patch(ctx, id, data, params).await
    }

    async fn remove(&self, ctx: &TenantContext, id: Option<&str>, params: BlogParams) -> Result<Value> {
        self.adapter._remove(ctx, id, params).await
    }
}

impl PostsService {
    pub fn new(state: Arc<BlogState>) -> Self {
        Self {
            adapter: BlogAdapter {
                state,
                store: StoreKind::Posts,
                id_prefix: "post",
                not_found_prefix: "Post not found",
                capabilities: posts_shared::crud_capabilities(),
            },
        }
    }
}
