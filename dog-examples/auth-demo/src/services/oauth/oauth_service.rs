
use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::{json, Map, Value};

use dog_auth::core::AuthenticationParams;
use dog_auth::AuthenticationService;
use dog_auth_oauth::OAuthService;
use dog_core::HookContext;
use dog_core::ServiceCaller;
use dog_core::ServiceMethodKind;
use crate::services::AuthDemoParams;

use super::oauth_shared;

pub struct OauthService {
    app: dog_core::DogApp<Value, AuthDemoParams>,
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
                let url = self
                    .app
                    .get::<String>("oauth.google.authorize_url")
                    .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.authorize_url in app config"))?;
                Ok(json!({ "location": url }))
            }
            "google_callback" => {
                let auth = AuthenticationService::from_app(&self.app)
                    .ok_or_else(|| anyhow::anyhow!("AuthenticationService missing from app state"))?;

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
                    return Err(anyhow::anyhow!("Missing code").into());
                }

                let auth_params = AuthenticationParams {
                    payload: None,
                    jwt_options: None,
                    auth_strategies: None,
                    secret: None,
                    headers: params.headers.clone(),
                };

                let services = ServiceCaller::new(self.app.clone());
                let config = self.app.config_snapshot();
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
            _ => Err(anyhow::anyhow!("Unknown oauth custom method: {method}").into()),
        }
    }
}

impl OauthService {
    pub fn new(app: dog_core::DogApp<Value, AuthDemoParams>) -> Self {
        Self { app }
    }
}
