use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, HookContext, ServiceCaller, ServiceCapabilities, ServiceMethodKind};
use serde_json::Value;

use crate::core::{AuthenticationParams, AuthenticationRequest};
use crate::hooks::authenticate::AuthenticateHookParams;
use crate::service::AuthenticationService;

pub struct AuthServiceAdapter<P>
where
    P: AuthenticateHookParams + Clone + Send + Sync + 'static,
{
    auth: Arc<AuthenticationService<P>>,
    app: std::sync::OnceLock<dog_core::DogApp<Value, P>>,
}

impl<P> AuthServiceAdapter<P>
where
    P: AuthenticateHookParams + Clone + Send + Sync + 'static,
{
    pub fn new(auth: Arc<AuthenticationService<P>>) -> Self {
        Self { 
            auth,
            app: std::sync::OnceLock::new(),
        }
    }

    pub fn setup(&self, app: dog_core::DogApp<Value, P>) {
        let _ = self.app.set(app);
    }

    pub fn auth(&self) -> &Arc<AuthenticationService<P>> {
        &self.auth
    }
}

#[async_trait]
impl<P> DogService<Value, P> for AuthServiceAdapter<P>
where
    P: AuthenticateHookParams + Clone + Send + Sync + 'static,
{
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::from_methods(vec![ServiceMethodKind::Create, ServiceMethodKind::Remove])
    }

    async fn create(&self, ctx: &TenantContext, data: Value, params: P) -> Result<Value> {
        let auth_req: AuthenticationRequest = serde_json::from_value(data)?;
        let strategies = self.auth.base.strategy_names();

        let auth_params = AuthenticationParams {
            payload: None,
            jwt_options: None,
            auth_strategies: Some(strategies.clone()),
            secret: None,
            headers: params.headers().clone(),
        };

        let app = self.app.get().expect("AuthServiceAdapter must be setup with DogApp");
        let services = ServiceCaller::new(app.clone());
        let config = app.config_snapshot();
        let mut hook_ctx = HookContext::new(ctx.clone(), ServiceMethodKind::Create, params, services, config);

        self.auth
            .create(&auth_req, &auth_params, &mut hook_ctx, &strategies, None)
            .await
    }

    async fn remove(&self, ctx: &TenantContext, id: Option<&str>, params: P) -> Result<Value> {
        let strategies = self.auth.base.strategy_names();

        let auth_params = AuthenticationParams {
            payload: None,
            jwt_options: None,
            auth_strategies: Some(strategies.clone()),
            secret: None,
            headers: params.headers().clone(),
        };

        let app = self.app.get().expect("AuthServiceAdapter must be setup with DogApp");
        let services = ServiceCaller::new(app.clone());
        let config = app.config_snapshot();
        let mut hook_ctx = HookContext::new(ctx.clone(), ServiceMethodKind::Remove, params, services, config);

        self.auth.remove(id, &auth_params, &mut hook_ctx, &strategies).await
    }
}
