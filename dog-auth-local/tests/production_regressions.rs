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

#[tokio::test]
async fn password_limit_is_bytes_and_never_silently_truncates() {
    let strategy = LocalStrategy::<()>::new().with_options(LocalStrategyOptions {
        hash_size: 4,
        ..Default::default()
    });
    let prefix = "a".repeat(72);
    assert!(strategy.hash_password(&prefix).await.is_ok());
    assert!(strategy.hash_password(&format!("{prefix}x")).await.is_err());
    assert!(strategy.hash_password(&"é".repeat(37)).await.is_err());
    assert!(strategy.hash_password("").await.is_err());
    assert!(strategy.hash_password("  ").await.is_err());
}
#[test]
fn excessive_password_cost_is_rejected_before_work() {
    for hash_size in [0, 3, 17, 31] {
        assert!(LocalStrategy::<()>::new()
            .with_options(LocalStrategyOptions {
                hash_size,
                ..Default::default()
            })
            .verify_configuration()
            .is_err());
    }
}

#[tokio::test]
async fn login_rejects_a_long_password_even_when_its_prefix_matches() {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    let base = AuthenticationService::builder(&mut builder, None)
        .unwrap()
        .build();
    let app = builder.build();
    let prefix = "a".repeat(72);
    let stored = bcrypt::hash(&prefix, 4).unwrap();
    let strategy = LocalStrategy::new()
        .with_entity_resolver(Arc::new(Resolver(json!({"id":"u","password":stored}))));
    let request =
        serde_json::from_value(json!({"email":"user","password":format!("{prefix}x")})).unwrap();
    let mut ctx = HookContext::new(
        TenantContext::new("test"),
        ServiceMethodKind::Create,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    assert!(strategy
        .authenticate(&request, &AuthenticationParams::default(), &mut ctx, &base)
        .await
        .is_err());
}
