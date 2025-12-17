use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;

use crate::services::adapters::blog_adapter::{StoreKind, TenantCrudService};
use crate::services::{BlogParams, BlogState};

use super::authors_shared;

pub struct AuthorsService {
    pub adapter: TenantCrudService,
}

#[async_trait]
impl DogService<Value, BlogParams> for AuthorsService {
    fn capabilities(&self) -> ServiceCapabilities {
        authors_shared::crud_capabilities()
    }

    async fn create(&self, ctx: &TenantContext, data: Value, params: BlogParams) -> Result<Value> {
        self.adapter._create(ctx, data, params).await
    }

    async fn find(&self, ctx: &TenantContext, params: BlogParams) -> Result<Vec<Value>> {
        self.adapter._find(ctx, params).await
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

impl AuthorsService {
    pub fn new(state: Arc<BlogState>) -> Self {
        Self {
            adapter: TenantCrudService {
                state,
                store: StoreKind::Authors,
                id_prefix: "author",
                not_found_prefix: "Author not found",
                capabilities: authors_shared::crud_capabilities(),
            },
        }
    }
}
