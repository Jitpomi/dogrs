use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use typedb_driver::{Credentials, DriverOptions, TypeDBDriver};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type CreateHandler<R, P> =
    Arc<dyn for<'a> Fn(&'a TenantContext, &'a TypeDBDriver, &'a str, R, P) -> BoxFuture<'a, Result<R>> + Send + Sync>;

pub type FindHandler<R, P> = Arc<
    dyn for<'a> Fn(&'a TenantContext, &'a TypeDBDriver, &'a str, P) -> BoxFuture<'a, Result<Vec<R>>>
        + Send
        + Sync,
>;

pub type GetHandler<R, P> = Arc<
    dyn for<'a> Fn(&'a TenantContext, &'a TypeDBDriver, &'a str, &'a str, P) -> BoxFuture<'a, Result<R>>
        + Send
        + Sync,
>;

pub type UpdateHandler<R, P> = Arc<
    dyn for<'a> Fn(&'a TenantContext, &'a TypeDBDriver, &'a str, &'a str, R, P) -> BoxFuture<'a, Result<R>>
        + Send
        + Sync,
>;

pub type PatchHandler<R, P> = Arc<
    dyn for<'a> Fn(
            &'a TenantContext,
            &'a TypeDBDriver,
            &'a str,
            Option<&'a str>,
            R,
            P,
        ) -> BoxFuture<'a, Result<R>>
        + Send
        + Sync,
>;

pub type RemoveHandler<R, P> = Arc<
    dyn for<'a> Fn(&'a TenantContext, &'a TypeDBDriver, &'a str, Option<&'a str>, P) -> BoxFuture<'a, Result<R>>
        + Send
        + Sync,
>;

#[derive(Default)]
pub struct TypeDBServiceHandlers<R, P> {
    pub create: Option<CreateHandler<R, P>>,
    pub find: Option<FindHandler<R, P>>,
    pub get: Option<GetHandler<R, P>>,
    pub update: Option<UpdateHandler<R, P>>,
    pub patch: Option<PatchHandler<R, P>>,
    pub remove: Option<RemoveHandler<R, P>>,
}

#[derive(Clone)]
pub struct TypeDBService {
    pub driver: Arc<TypeDBDriver>,
    pub database: String,
    pub capabilities: ServiceCapabilities,
    pub handlers: Arc<TypeDBServiceHandlers<serde_json::Value, serde_json::Value>>,
}

impl TypeDBService {
    pub fn new(driver: Arc<TypeDBDriver>, database: impl Into<String>) -> Self {
        Self {
            driver,
            database: database.into(),
            capabilities: ServiceCapabilities::standard_crud(),
            handlers: Arc::new(TypeDBServiceHandlers::default()),
        }
    }

    pub fn with_handlers(
        driver: Arc<TypeDBDriver>,
        database: impl Into<String>,
        capabilities: ServiceCapabilities,
        handlers: TypeDBServiceHandlers<serde_json::Value, serde_json::Value>,
    ) -> Self {
        Self {
            driver,
            database: database.into(),
            capabilities,
            handlers: Arc::new(handlers),
        }
    }
}

#[async_trait]
impl DogService<serde_json::Value, serde_json::Value> for TypeDBService {
    fn capabilities(&self) -> ServiceCapabilities {
        self.capabilities.clone()
    }

    async fn create(
        &self,
        ctx: &TenantContext,
        data: serde_json::Value,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let handler = self
            .handlers
            .create
            .as_ref()
            .ok_or_else(|| anyhow!("Method not implemented: create"))?;

        handler(ctx, &self.driver, &self.database, data, params).await
    }

    async fn find(&self, ctx: &TenantContext, params: serde_json::Value) -> Result<Vec<serde_json::Value>> {
        let handler = self
            .handlers
            .find
            .as_ref()
            .ok_or_else(|| anyhow!("Method not implemented: find"))?;

        handler(ctx, &self.driver, &self.database, params).await
    }

    async fn get(&self, ctx: &TenantContext, id: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let handler = self
            .handlers
            .get
            .as_ref()
            .ok_or_else(|| anyhow!("Method not implemented: get"))?;

        handler(ctx, &self.driver, &self.database, id, params).await
    }

    async fn update(
        &self,
        ctx: &TenantContext,
        id: &str,
        data: serde_json::Value,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let handler = self
            .handlers
            .update
            .as_ref()
            .ok_or_else(|| anyhow!("Method not implemented: update"))?;

        handler(ctx, &self.driver, &self.database, id, data, params).await
    }

    async fn patch(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        data: serde_json::Value,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let handler = self
            .handlers
            .patch
            .as_ref()
            .ok_or_else(|| anyhow!("Method not implemented: patch"))?;

        handler(ctx, &self.driver, &self.database, id, data, params).await
    }

    async fn remove(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let handler = self
            .handlers
            .remove
            .as_ref()
            .ok_or_else(|| anyhow!("Method not implemented: remove"))?;

        handler(ctx, &self.driver, &self.database, id, params).await
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        _method: &str,
        _data: Option<serde_json::Value>,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        Err(anyhow!("Custom methods not implemented by this TypeDB service"))
    }
}

pub struct TypeDBDriverFactory;

impl TypeDBDriverFactory {
    pub async fn connect(address: &str, username: &str, password: &str, tls: bool) -> Result<TypeDBDriver> {
        let options = DriverOptions::new(tls, None).map_err(|e| anyhow!(e))?;
        TypeDBDriver::new(address, Credentials::new(username, password), options)
            .await
            .map_err(|e| anyhow!(e))
    }

    pub async fn connect_default(address: &str) -> Result<TypeDBDriver> {
        Self::connect(address, "admin", "password", false).await
    }
}
