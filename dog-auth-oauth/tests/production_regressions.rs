use async_trait::async_trait;
use dog_auth::core::{AuthenticationParams, AuthenticationRequest, AuthenticationStrategy};
use dog_auth::{AuthOptions, AuthenticationService};
use dog_auth_oauth::{OAuthProvider, OAuthStrategy};
use dog_core::{
    tenant::TenantContext, DogAppBuilder, HookContext, ServiceCaller, ServiceMethodKind,
};
use serde_json::{json, Value};
use std::sync::Arc;
struct Provider;
#[async_trait]
impl OAuthProvider<()> for Provider {
    fn supports_access_token_login(&self) -> bool {
        true
    }
    fn name(&self) -> &str {
        "test"
    }
    async fn exchange_code(
        &self,
        code: &str,
        state: Option<&str>,
        _: &mut HookContext<Value, ()>,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            code == "valid-code" && state == Some("verified-state"),
            "Invalid callback"
        );
        Ok("valid-token".into())
    }
    async fn fetch_profile(
        &self,
        token: &str,
        _: &mut HookContext<Value, ()>,
    ) -> anyhow::Result<Option<Value>> {
        Ok((token == "valid-token").then(|| json!({"sub":"verified-user"})))
    }
}
async fn authenticate(request: Value) -> anyhow::Result<Value> {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    let auth = AuthenticationService::builder(&mut builder, Some(AuthOptions::default()))
        .unwrap()
        .build();
    let app = builder.build();
    let strategy = OAuthStrategy::new().register_provider(Arc::new(Provider));
    let request: AuthenticationRequest = serde_json::from_value(request).unwrap();
    let mut ctx = HookContext::new(
        TenantContext::new("test"),
        ServiceMethodKind::Create,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    strategy
        .authenticate(&request, &AuthenticationParams::default(), &mut ctx, &auth)
        .await
}
#[tokio::test]
async fn caller_cannot_supply_identity_or_skip_token_validation() {
    assert!(
        authenticate(json!({"provider":"test","profile":{"sub":"victim"}}))
            .await
            .is_err()
    );
    assert!(authenticate(
        json!({"provider":"test","accessToken":"invalid","profile":{"sub":"victim"}})
    )
    .await
    .is_err());
    assert!(
        authenticate(json!({"provider":"test","accessToken":"invalid"}))
            .await
            .is_err()
    );
    assert!(
        authenticate(json!({"provider":"unknown","accessToken":"valid-token"}))
            .await
            .is_err()
    );
    let out = authenticate(json!({"provider":"test","accessToken":"valid-token"}))
        .await
        .unwrap();
    assert_eq!(out["profile"]["sub"], "verified-user");
    assert!(out["authentication"].get("accessToken").is_none());
}
#[tokio::test]
async fn code_exchange_receives_callback_state() {
    assert!(authenticate(json!({"provider":"test","code":"valid-code"}))
        .await
        .is_err());
    let out = authenticate(json!({"provider":"test","code":"valid-code","state":"verified-state"}))
        .await
        .unwrap();
    assert_eq!(out["profile"]["sub"], "verified-user");
    assert!(out["authentication"].get("code").is_none());
}

struct RejectEntity;
#[async_trait]
impl dog_auth_oauth::OAuthEntityResolver<()> for RejectEntity {
    async fn resolve_entity(
        &self,
        _: &str,
        _: &Value,
        _: &mut HookContext<Value, ()>,
    ) -> anyhow::Result<Option<Value>> {
        Ok(None)
    }
}
#[tokio::test]
async fn configured_entity_resolver_can_reject_login() {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    let options = AuthOptions {
        entity: Some("user".into()),
        service: Some("users".into()),
        ..Default::default()
    };
    let auth = AuthenticationService::builder(&mut builder, Some(options))
        .unwrap()
        .build();
    let app = builder.build();
    let strategy = OAuthStrategy::new()
        .register_provider(Arc::new(Provider))
        .with_entity_resolver(Arc::new(RejectEntity));
    let request =
        serde_json::from_value(json!({"provider":"test","accessToken":"valid-token"})).unwrap();
    let mut ctx = HookContext::new(
        TenantContext::new("test"),
        ServiceMethodKind::Create,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    assert!(strategy
        .authenticate(&request, &AuthenticationParams::default(), &mut ctx, &auth)
        .await
        .is_err());
}

#[tokio::test]
async fn entity_mapping_requires_an_explicit_atomic_resolver() {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    let auth = AuthenticationService::builder(
        &mut builder,
        Some(AuthOptions {
            entity: Some("user".into()),
            service: Some("users".into()),
            ..Default::default()
        }),
    )
    .unwrap()
    .build();
    let app = builder.build();
    let strategy = OAuthStrategy::new().register_provider(Arc::new(Provider));
    let request =
        serde_json::from_value(json!({"provider":"test","accessToken":"valid-token"})).unwrap();
    let mut ctx = HookContext::new(
        TenantContext::new("test"),
        ServiceMethodKind::Create,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    let error = strategy
        .authenticate(&request, &AuthenticationParams::default(), &mut ctx, &auth)
        .await
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<dog_core::DogError>().unwrap().code(),
        503
    );
    assert!(error.to_string().contains("atomic entity resolver"));
}

struct CodeOnly;
#[async_trait]
impl OAuthProvider<()> for CodeOnly {
    fn name(&self) -> &str {
        "test"
    }
    async fn exchange_code(
        &self,
        _: &str,
        _: Option<&str>,
        _: &mut HookContext<Value, ()>,
    ) -> anyhow::Result<String> {
        Err(anyhow::anyhow!("provider-secret-must-not-escape"))
    }
    async fn fetch_profile(
        &self,
        _: &str,
        _: &mut HookContext<Value, ()>,
    ) -> anyhow::Result<Option<Value>> {
        panic!("disabled raw-token flow must not fetch a profile")
    }
}
#[tokio::test]
async fn raw_token_login_requires_opt_in_and_provider_errors_are_sanitized() {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    let auth = AuthenticationService::builder(&mut builder, None)
        .unwrap()
        .build();
    let app = builder.build();
    let strategy = OAuthStrategy::new().register_provider(Arc::new(CodeOnly));
    let mut ctx = HookContext::new(
        TenantContext::new("test"),
        ServiceMethodKind::Create,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    for data in [
        json!({"provider":"test","accessToken":"token"}),
        json!({"provider":"test","code":"code","state":"state"}),
    ] {
        let error = strategy
            .authenticate(
                &serde_json::from_value(data).unwrap(),
                &AuthenticationParams::default(),
                &mut ctx,
                &auth,
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<dog_core::DogError>().unwrap().code(),
            401
        );
        assert!(!error.to_string().contains("provider-secret"));
    }
}
