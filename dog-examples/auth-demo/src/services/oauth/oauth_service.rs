use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::{json, Map, Value};
use std::sync::Arc;

use crate::services::AuthDemoParams;
use dog_auth::core::AuthenticationParams;
use dog_auth::AuthenticationService;
use dog_auth_oauth::OAuthService;
use dog_core::HookContext;
use dog_core::ServiceCaller;
use dog_core::ServiceMethodKind;

use super::oauth_shared;

pub struct OauthService {
    auth: Arc<AuthenticationService<AuthDemoParams>>,
    app: std::sync::OnceLock<dog_core::DogApp<Value, AuthDemoParams>>,
}

#[async_trait]
impl DogService<Value, AuthDemoParams> for OauthService {
    fn capabilities(&self) -> ServiceCapabilities {
        oauth_shared::crud_capabilities()
    }

    async fn custom(
        &self,
        tenant: &TenantContext,
        method: &str,
        data: Option<Value>,
        params: AuthDemoParams,
    ) -> Result<Value> {
        match method {
            "google_login" => {
                let app = self
                    .app
                    .get()
                    .ok_or_else(|| anyhow::anyhow!("DogApp not setup"))?;
                let url = app
                    .get::<String>("oauth.google.authorize_url")
                    .ok_or_else(|| {
                        anyhow::anyhow!("Missing oauth.google.authorize_url in app config")
                    })?;
                Ok(json!({ "location": url }))
            }
            "google_callback" => {
                let app = self
                    .app
                    .get()
                    .ok_or_else(|| anyhow::anyhow!("DogApp not setup"))?;
                let auth = Arc::clone(&self.auth);

                let provider = data
                    .as_ref()
                    .and_then(|v| v.get("provider"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("google");

                let code = data
                    .as_ref()
                    .and_then(|v| v.get("code"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if code.trim().is_empty() {
                    return Err(anyhow::anyhow!("Missing code"));
                }

                let auth_params = AuthenticationParams {
                    payload: None,
                    jwt_options: None,
                    auth_strategies: None,
                    secret: None,
                    headers: params.headers.clone(),
                };

                let services = ServiceCaller::new(app.clone());
                let config = app.config_snapshot();
                let mut hook_ctx = HookContext::new(
                    tenant.clone(),
                    ServiceMethodKind::Create,
                    params,
                    services,
                    config,
                );

                let mut payload: Map<String, Value> = Map::new();
                payload.insert("provider".to_string(), Value::String(provider.to_string()));
                payload.insert("code".to_string(), Value::String(code.to_string()));

                let res = OAuthService::new(auth)
                    .authenticate_callback("oauth", payload, &auth_params, &mut hook_ctx, None)
                    .await?;

                Ok(res.auth_result)
            }
            _ => Err(anyhow::anyhow!("Unknown oauth custom method: {method}")),
        }
    }
}

impl OauthService {
    pub fn new(auth: Arc<AuthenticationService<AuthDemoParams>>) -> Self {
        Self {
            auth,
            app: std::sync::OnceLock::new(),
        }
    }

    pub fn setup(&self, app: dog_core::DogApp<Value, AuthDemoParams>) {
        let _ = self.app.set(app);
    }
}
