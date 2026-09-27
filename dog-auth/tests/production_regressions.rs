use dog_auth::hooks::{AuthParams, AuthenticateHook};
use dog_auth::{AuthOptions, AuthenticationService};
use dog_core::DogAppBuilder;
use serde_json::{json, Value};
use std::sync::Arc;
type P = AuthParams<()>;
fn setup() -> (DogAppBuilder<Value, P>, Arc<AuthenticationService<P>>) {
    let mut builder = DogAppBuilder::new();
    let mut options = AuthOptions::default();
    options.jwt.secret = Some("audit-only-test-key-at-least-32-bytes-long".into());
    let base = AuthenticationService::builder(&mut builder, Some(options))
        .unwrap()
        .build();
    (
        builder,
        Arc::new(AuthenticationService::new(Arc::new(base))),
    )
}
#[tokio::test]
async fn access_verifier_must_reject_refresh_token() {
    let (_, auth) = setup();
    let token = auth
        .base
        .create_refresh_token(json!({"sub":"audit-user"}), None)
        .await
        .unwrap();
    assert!(
        auth.base.verify_access_token(&token).await.is_err(),
        "refresh token accepted as access token"
    );
}
#[test]
fn installed_auth_must_be_retrievable() {
    let (mut builder, auth) = setup();
    let adapter = AuthenticationService::install(&mut builder, auth);
    let app = builder.build();
    adapter.setup(app.clone());
    assert!(
        AuthenticationService::from_app(&app).is_some(),
        "from_app returns None after install and setup"
    );
    assert!(AuthenticateHook::from_app(&app, vec!["jwt".into()]).is_ok());
}
#[tokio::test]
async fn control_access_token_is_accepted_and_tampering_is_rejected() {
    let (_, auth) = setup();
    let token = auth
        .base
        .create_access_token(json!({"sub":"audit-user"}), None)
        .await
        .unwrap();
    assert_eq!(
        auth.base.verify_access_token(&token).await.unwrap()["sub"],
        "audit-user"
    );
    let mut bytes = token.into_bytes();
    let i = bytes.len() - 8;
    bytes[i] = if bytes[i] == b'A' { b'B' } else { b'A' };
    assert!(auth
        .base
        .verify_access_token(&String::from_utf8(bytes).unwrap())
        .await
        .is_err());
}

#[tokio::test]
async fn refresh_verifier_requires_refresh_tokens() {
    let (_, auth) = setup();
    let access = auth
        .base
        .create_access_token(json!({"sub":"audit-user"}), None)
        .await
        .unwrap();
    let refresh = auth
        .base
        .create_refresh_token(json!({"sub":"audit-user"}), None)
        .await
        .unwrap();
    assert!(auth.base.verify_refresh_token(&access).await.is_err());
    assert!(auth.base.verify_refresh_token(&refresh).await.is_ok());
}

#[cfg(feature = "jwt-pem")]
#[tokio::test]
async fn configured_rsa_and_ec_keys_sign_and_verify() {
    for (algorithm, name) in [
        (dog_auth::JwtAlgorithm::RS256, "rsa"),
        (dog_auth::JwtAlgorithm::ES256, "ec"),
    ] {
        let mut builder = DogAppBuilder::<Value, P>::new();
        let mut options = AuthOptions::default();
        options.jwt.algorithm = algorithm;
        options.jwt.private_key_path = Some(format!(
            "{}/tests/fixtures/test-{name}-private.pem",
            env!("CARGO_MANIFEST_DIR")
        ));
        options.jwt.public_key_path = Some(format!(
            "{}/tests/fixtures/test-{name}-public.pem",
            env!("CARGO_MANIFEST_DIR")
        ));
        let auth = AuthenticationService::builder(&mut builder, Some(options))
            .unwrap()
            .build();
        let auth = Arc::new(auth);
        AuthenticationService::new(auth.clone())
            .setup_validate()
            .await
            .unwrap();
        let token = auth
            .create_access_token(json!({"sub":"test"}), None)
            .await
            .unwrap();
        assert_eq!(
            auth.verify_access_token(&token)
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}"))["sub"],
            "test"
        );
    }
}

#[derive(Default)]
struct TestStore {
    revoked: std::sync::Mutex<std::collections::HashSet<(String, String)>>,
    fail: std::sync::atomic::AtomicBool,
}
#[async_trait::async_trait]
impl dog_auth::TokenStore for TestStore {
    async fn is_revoked(&self, issuer: &str, jti: &str) -> anyhow::Result<bool> {
        anyhow::ensure!(
            !self.fail.load(std::sync::atomic::Ordering::SeqCst),
            "store unavailable"
        );
        Ok(self
            .revoked
            .lock()
            .unwrap()
            .contains(&(issuer.into(), jti.into())))
    }
    async fn revoke(&self, issuer: &str, jti: &str, _: i64) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.fail.load(std::sync::atomic::Ordering::SeqCst),
            "store unavailable"
        );
        self.revoked
            .lock()
            .unwrap()
            .insert((issuer.into(), jti.into()));
        Ok(())
    }
    async fn consume_refresh(&self, issuer: &str, jti: &str, _: i64) -> anyhow::Result<bool> {
        anyhow::ensure!(
            !self.fail.load(std::sync::atomic::Ordering::SeqCst),
            "store unavailable"
        );
        Ok(self
            .revoked
            .lock()
            .unwrap()
            .insert((issuer.into(), jti.into())))
    }
}
fn stateful() -> (Arc<dog_auth::AuthenticationBase<P>>, Arc<TestStore>) {
    let mut app = DogAppBuilder::new();
    let mut options = AuthOptions::default();
    options.jwt.secret = Some("audit-only-test-key-at-least-32-bytes-long".into());
    let store = Arc::new(TestStore::default());
    let base = AuthenticationService::builder(&mut app, Some(options))
        .unwrap()
        .with_token_store(store.clone())
        .build();
    (Arc::new(base), store)
}
#[tokio::test]
async fn concurrent_refresh_rotation_has_one_winner_and_replay_fails() {
    let (auth, _) = stateful();
    let token = auth
        .create_refresh_token(json!({"sub":"alice"}), None)
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..32 {
        let auth = auth.clone();
        let token = token.clone();
        tasks.spawn(async move { auth.rotate_refresh_token(&token).await });
    }
    let mut winners = 0;
    while let Some(result) = tasks.join_next().await {
        if let Ok(pair) = result.unwrap() {
            winners += 1;
            assert_eq!(
                auth.verify_access_token(&pair.access_token).await.unwrap()["sub"],
                "alice"
            );
            assert!(auth.verify_refresh_token(&pair.refresh_token).await.is_ok());
        }
    }
    assert_eq!(winners, 1);
    assert!(auth.verify_refresh_token(&token).await.is_err());
    assert!(auth.rotate_refresh_token(&token).await.is_err());
}
#[tokio::test]
async fn revocation_and_store_failure_fail_closed() {
    let (auth, store) = stateful();
    let token = auth
        .create_access_token(json!({"sub":"alice"}), None)
        .await
        .unwrap();
    auth.revoke_access_token(&token).await.unwrap();
    assert!(auth.verify_access_token(&token).await.is_err());
    let refresh = auth
        .create_refresh_token(json!({"sub":"alice"}), None)
        .await
        .unwrap();
    auth.revoke_refresh_token(&refresh).await.unwrap();
    assert!(auth.rotate_refresh_token(&refresh).await.is_err());
    let fresh = auth
        .create_access_token(json!({"sub":"alice"}), None)
        .await
        .unwrap();
    let refresh = auth
        .create_refresh_token(json!({"sub":"alice"}), None)
        .await
        .unwrap();
    store.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(auth.verify_access_token(&fresh).await.is_err());
    assert!(auth.revoke_access_token(&fresh).await.is_err());
    assert!(auth.rotate_refresh_token(&refresh).await.is_err());
}
#[tokio::test]
async fn stateless_mode_cannot_claim_revocation_or_rotation() {
    let (_, auth) = setup();
    let access = auth
        .base
        .create_access_token(json!({}), None)
        .await
        .unwrap();
    let refresh = auth
        .base
        .create_refresh_token(json!({}), None)
        .await
        .unwrap();
    assert!(auth.base.revoke_access_token(&access).await.is_err());
    assert!(auth.base.rotate_refresh_token(&refresh).await.is_err());
}
#[tokio::test]
async fn invalid_lifetimes_secrets_and_reserved_claims_are_rejected() {
    let (_, auth) = setup();
    for lifetime in [0, u64::MAX] {
        assert!(auth
            .base
            .create_access_token(
                json!({}),
                Some(dog_auth::JwtOverrides {
                    expires_in_seconds: Some(lifetime),
                    ..Default::default()
                })
            )
            .await
            .is_err());
    }
    for secret in ["", "short"] {
        let mut options = dog_auth::JwtOptions {
            secret: Some(secret.into()),
            ..Default::default()
        };
        assert!(options.validate().is_err());
        options.secret = Some("a".repeat(32));
        options.custom_claims.insert("exp".into(), json!(0));
        assert!(options.validate().is_err());
    }
}
#[tokio::test]
async fn jwt_requires_exp_issuer_audience_jti_and_enforces_nbf() {
    let (_, auth) = setup();
    let now = chrono::Utc::now().timestamp();
    let valid =
        json!({"iss":"dogrs-auth","aud":["dogrs-api"],"exp":now+300,"iat":now,"jti":"unique"});
    let mut invalid = Vec::new();
    for key in ["exp", "iss", "aud", "jti"] {
        let mut claims = valid.clone();
        claims.as_object_mut().unwrap().remove(key);
        invalid.push(claims);
    }
    for (key, value) in [
        ("exp", json!(now - 1)),
        ("exp", json!(now)),
        ("nbf", json!(now + 60)),
        ("aud", json!(["wrong"])),
        ("iss", json!("wrong")),
    ] {
        let mut claims = valid.clone();
        claims[key] = value;
        invalid.push(claims);
    }
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    header.typ = Some("access".into());
    for claims in invalid {
        let token = jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(b"audit-only-test-key-at-least-32-bytes-long"),
        )
        .unwrap();
        assert!(
            auth.base.verify_access_token(&token).await.is_err(),
            "accepted {claims}"
        );
    }
}

#[tokio::test]
async fn service_logout_revokes_the_supplied_token() {
    let mut builder = DogAppBuilder::<Value, P>::new();
    let mut options = AuthOptions::default();
    options.jwt.secret = Some("audit-only-test-key-at-least-32-bytes-long".into());
    let mut auth = AuthenticationService::builder(&mut builder, Some(options))
        .unwrap()
        .with_token_store(Arc::new(TestStore::default()));
    auth.register("jwt", Arc::new(dog_auth::JwtStrategy::<P>::new()));
    let service = AuthenticationService::new(Arc::new(auth.build()));
    let token = service
        .base
        .create_access_token(json!({"sub":"alice"}), None)
        .await
        .unwrap();
    let app = builder.build();
    let mut ctx = dog_core::HookContext::new(
        dog_core::TenantContext::new("test"),
        dog_core::ServiceMethodKind::Remove,
        P::default(),
        dog_core::ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    service
        .remove(
            Some(&token),
            &dog_auth::AuthenticationParams::default(),
            &mut ctx,
            &["jwt".into()],
        )
        .await
        .unwrap();
    assert!(service.base.verify_access_token(&token).await.is_err());
}
