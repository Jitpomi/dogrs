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
