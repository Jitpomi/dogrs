use anyhow::Result;
use dog_core::DogApp;
use dog_transport::{http::DogHttpService, HttpOptions, IntoDogService};
use serde_json::Value;

use crate::services::AuthDemoParams;

pub async fn auth_app() -> Result<(
    DogApp<Value, AuthDemoParams>,
    DogHttpService<Value, AuthDemoParams>,
)> {
    dotenvy::from_filename("dog-examples/auth-demo/.env").ok();
    dotenvy::dotenv().ok();

    let mut builder: dog_core::DogAppBuilder<Value, AuthDemoParams> =
        dog_core::DogAppBuilder::new();

    crate::config::config(&mut builder)?;
    compose(builder)
}

fn compose(
    mut builder: dog_core::DogAppBuilder<Value, AuthDemoParams>,
) -> Result<(
    DogApp<Value, AuthDemoParams>,
    DogHttpService<Value, AuthDemoParams>,
)> {
    let auth_adapter = crate::auth::strategies(&mut builder)?;
    let oauth_raw = crate::services::configure(&mut builder, auth_adapter.clone())?;
    crate::hooks::global_hooks(&mut builder);
    crate::channels::configure(&mut builder)?;

    let dog_app = builder.build();
    auth_adapter.setup(dog_app.clone());
    oauth_raw.setup(dog_app.clone());

    let http_service = dog_app.clone().into_service(
        HttpOptions::default()
            .tenant_header("x-tenant-id")
            .enable_cors(true)
            .route("/messages", "messages")
            .route("/users", "users")
            .route("/oauth", "oauth"),
    );

    Ok((dog_app, http_service))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::{BodyExt, Full};
    use serde_json::json;
    use tower::ServiceExt;
    async fn call(
        service: DogHttpService<Value, AuthDemoParams>,
        method: &str,
        path: &str,
        data: Value,
        token: Option<&str>,
    ) -> (u16, Value) {
        let mut request = http::Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = service
            .oneshot(
                request
                    .body(Full::new(dog_transport::bytes::Bytes::from(
                        serde_json::to_vec(&data).unwrap(),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
    #[tokio::test]
    async fn login_ownership_logout_and_identity_fields() {
        let directory = tempfile::tempdir().unwrap();
        let mut builder = dog_core::DogAppBuilder::new();
        builder.set(
            "auth.jwt.secret",
            "example-test-only-random-length-secret-123456789",
        );
        builder.set("auth.service", "users");
        builder.set("auth.entity", "user");
        builder.set(
            "auth.token_store.directory",
            directory.path().to_string_lossy().to_string(),
        );
        let (_, service) = compose(builder).unwrap();
        let (status, alice) = call(
            service.clone(),
            "POST",
            "/users",
            json!({"username":"alice", "password":"password-test"}),
            None,
        )
        .await;
        assert!((200..300).contains(&status), "{status}: {alice}");
        assert!(alice.get("password").is_none());
        let (_, bob) = call(
            service.clone(),
            "POST",
            "/users",
            json!({"username":"bob", "password":"password-test"}),
            None,
        )
        .await;
        let (status, login) = call(
            service.clone(),
            "POST",
            "/authentication",
            json!({"strategy":"local", "username":"alice", "password":"password-test"}),
            None,
        )
        .await;
        assert!((200..300).contains(&status), "{status}: {login}");
        let token = login["accessToken"].as_str().unwrap();
        let (status, _) = call(
            service.clone(),
            "PUT",
            "/messages/anything",
            json!({"text":"unauthenticated", "sender":alice["id"]}),
            None,
        )
        .await;
        assert_eq!(status, 401);
        let (status, _) = call(
            service.clone(),
            "POST",
            "/users",
            json!({"username":"forged", "password":"password-test", "googleId":"victim"}),
            None,
        )
        .await;
        assert_eq!(status, 403);
        let (status, message) = call(
            service.clone(),
            "POST",
            "/messages",
            json!({"text":"hello", "sender":alice["id"]}),
            Some(token),
        )
        .await;
        assert!((200..300).contains(&status), "{status}: {message}");
        let (_, bob_login) = call(
            service.clone(),
            "POST",
            "/authentication",
            json!({"strategy":"local", "username":"bob", "password":"password-test"}),
            None,
        )
        .await;
        let (status, _) = call(
            service.clone(),
            "PATCH",
            &format!("/messages/{}", message["id"].as_str().unwrap()),
            json!({"text":"not mine"}),
            bob_login["accessToken"].as_str(),
        )
        .await;
        assert_eq!(status, 403);

        let (status, _) = call(service.clone(), "GET", "/users", Value::Null, None).await;
        assert_eq!(status, 401);
        let (status, _) = call(
            service.clone(),
            "PATCH",
            &format!("/users/{}", bob["id"].as_str().unwrap()),
            json!({"username":"stolen"}),
            Some(token),
        )
        .await;
        assert_eq!(status, 403);
        let (status, _) = call(
            service.clone(),
            "PATCH",
            &format!("/users/{}", alice["id"].as_str().unwrap()),
            json!({"googleId":"forged"}),
            Some(token),
        )
        .await;
        assert_eq!(status, 403);
        let (status, _) = call(
            service.clone(),
            "DELETE",
            "/authentication",
            Value::Null,
            Some(token),
        )
        .await;
        assert!((200..300).contains(&status), "logout: {status}");
        let (status, _) = call(
            service,
            "GET",
            &format!("/users/{}", alice["id"].as_str().unwrap()),
            Value::Null,
            Some(token),
        )
        .await;
        assert_eq!(status, 401);
    }
}
