use async_trait::async_trait;
use dog_auth::core::{AuthenticationParams, AuthenticationRequest, AuthenticationStrategy};
use dog_auth::{AuthOptions, AuthenticationService};
use dog_auth_local::{LocalEntityResolver, LocalStrategy, LocalStrategyOptions};
use dog_core::{
    tenant::TenantContext, DogAppBuilder, HookContext, ServiceCaller, ServiceMethodKind,
};
use serde_json::{json, Value};
use std::sync::Arc;
struct Resolver(Value);
#[async_trait]
impl LocalEntityResolver<()> for Resolver {
    async fn resolve_entity(
        &self,
        _: &str,
        _: &mut HookContext<Value, ()>,
    ) -> anyhow::Result<Option<Value>> {
        Ok(Some(self.0.clone()))
    }
}
#[tokio::test]
async fn nested_password_hash_must_not_be_returned() {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    let base = AuthenticationService::builder(&mut builder, Some(AuthOptions::default()))
        .unwrap()
        .build();
    let app = builder.build();
    let hash = bcrypt::hash("audit-password", 4).unwrap();
    let strategy = LocalStrategy::new()
        .with_options(LocalStrategyOptions {
            entity_password_field: "credentials.hash".into(),
            ..Default::default()
        })
        .with_entity_resolver(Arc::new(Resolver(
            json!({"id":"test-user","credentials":{"hash":hash}}),
        )));
    let req: AuthenticationRequest = serde_json::from_value(
        json!({"strategy":"local","email":"audit@example.invalid","password":"audit-password"}),
    )
    .unwrap();
    let mut ctx = HookContext::new(
        TenantContext::new("audit"),
        ServiceMethodKind::Create,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    let out = strategy
        .authenticate(&req, &AuthenticationParams::default(), &mut ctx, &base)
        .await
        .unwrap();
    assert!(
        out.pointer("/user/credentials/hash").is_none(),
        "nested password hash exposed in authentication result"
    );
}
