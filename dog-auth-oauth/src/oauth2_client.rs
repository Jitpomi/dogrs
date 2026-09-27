use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::HookContext;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde_json::Value;

use crate::strategy::OAuthProvider;

// ---------------------------------------------------------------------------
// Type alias for the configured client with both auth_uri AND token_uri set.
// oauth2 5.x uses type-state generics: each endpoint tracks Set/NotSet.
// Parameters (in order): HasAuthUrl, HasDeviceAuthUrl, HasIntrospectionUrl,
//                         HasRevocationUrl, HasTokenUrl
// ---------------------------------------------------------------------------
type ConfiguredBasicClient = BasicClient<
    EndpointSet,    // HasAuthUrl    — set via .set_auth_uri()
    EndpointNotSet, // HasDeviceAuthUrl
    EndpointNotSet, // HasIntrospectionUrl
    EndpointNotSet, // HasRevocationUrl
    EndpointSet,    // HasTokenUrl   — set via .set_token_uri()
>;

pub struct OAuth2ClientConfig {
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub userinfo_url: Option<String>,
}

/// Store `state` and `code_verifier` server-side, bound to the initiating browser
/// session with a short expiration. Never return the verifier to an API caller.
pub struct OAuthAuthorization {
    pub url: String,
    pub state: String,
    pub code_verifier: String,
}

#[async_trait]
pub trait OAuthCallbackVerifier<P>: Send + Sync
where
    P: Clone + Send + Sync + 'static,
{
    /// Validate browser/session binding and expiration, atomically consume the
    /// one-use state, then return its stored PKCE verifier. Reject missing/replayed state.
    async fn consume(&self, state: &str, ctx: &mut HookContext<Value, P>) -> Result<String>;
}

pub struct OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    name: String,
    client: ConfiguredBasicClient,
    scopes: Vec<String>,
    userinfo_url: Option<String>,
    verifier: Arc<dyn OAuthCallbackVerifier<P>>,
    http: reqwest::Client,
}

impl<P> OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub fn new(
        config: OAuth2ClientConfig,
        verifier: Arc<dyn OAuthCallbackVerifier<P>>,
    ) -> Result<Self> {
        for value in [&config.auth_url, &config.token_url]
            .into_iter()
            .chain(config.userinfo_url.as_ref())
        {
            validate_endpoint(value, false)?;
        }
        validate_endpoint(&config.redirect_uri, true)?;
        anyhow::ensure!(
            !config.client_id.trim().is_empty() && !config.client_secret.is_empty(),
            "OAuth credentials are required"
        );
        // oauth2 5.x: BasicClient::new() takes only ClientId; other fields via builders.
        // Each set_* call changes the type-state, giving us ConfiguredBasicClient.
        let client = BasicClient::new(ClientId::new(config.client_id))
            .set_client_secret(ClientSecret::new(config.client_secret))
            .set_auth_uri(AuthUrl::new(config.auth_url)?)
            .set_token_uri(TokenUrl::new(config.token_url)?)
            .set_redirect_uri(RedirectUrl::new(config.redirect_uri)?);

        Ok(Self {
            name: config.name,
            client,
            scopes: config.scopes,
            userinfo_url: config.userinfo_url,
            verifier,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(15))
                .build()?,
        })
    }

    pub fn authorize_url(&self) -> OAuthAuthorization {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut req = self
            .client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge);
        for scope in &self.scopes {
            req = req.add_scope(Scope::new(scope.clone()));
        }
        let (url, state) = req.url();
        OAuthAuthorization {
            url: url.to_string(),
            state: state.secret().clone(),
            code_verifier: verifier.secret().clone(),
        }
    }
}

#[async_trait]
impl<P> OAuthProvider<P> for OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    async fn exchange_code(
        &self,
        code: &str,
        state: Option<&str>,
        ctx: &mut HookContext<Value, P>,
    ) -> Result<String> {
        // oauth2 5.x with EndpointSet: exchange_code() returns CodeTokenRequest (not Result).
        // request_async takes a &reqwest::Client (implements AsyncHttpClient).
        let state = state.filter(|s| !s.is_empty()).ok_or_else(|| {
            dog_core::DogError::not_authenticated("Missing OAuth state").into_anyhow()
        })?;
        let verifier = self.verifier.consume(state, ctx).await?;
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(verifier))
            .request_async(&|request| bounded_request(self.http.clone(), request))
            .await?;

        Ok(token.access_token().secret().to_string())
    }

    async fn fetch_profile(
        &self,
        access_token: &str,
        _ctx: &mut HookContext<Value, P>,
    ) -> Result<Option<Value>> {
        let Some(url) = self.userinfo_url.as_deref() else {
            return Ok(None);
        };

        let response = self
            .http
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?;
        anyhow::ensure!(
            response.status().is_success(),
            "OAuth profile request failed"
        );
        let bytes = read_bounded(response).await?;
        Ok(Some(serde_json::from_slice(&bytes)?))
    }
}

async fn bounded_request(
    http: reqwest::Client,
    request: oauth2::HttpRequest,
) -> std::result::Result<oauth2::HttpResponse, std::io::Error> {
    async {
        let response = http.execute(reqwest::Request::try_from(request)?).await?;
        let mut builder = oauth2::http::Response::builder().status(response.status());
        for (name, value) in response.headers() {
            builder = builder.header(name, value);
        }
        Ok::<_, anyhow::Error>(builder.body(read_bounded(response).await?)?)
    }
    .await
    .map_err(|_| std::io::Error::other("OAuth HTTP request failed"))
}

const MAX_PROVIDER_RESPONSE: usize = 1024 * 1024;

async fn read_bounded(mut response: reqwest::Response) -> Result<Vec<u8>> {
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_PROVIDER_RESPONSE as u64),
        "OAuth response too large"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            chunk.len() <= MAX_PROVIDER_RESPONSE - bytes.len(),
            "OAuth response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_endpoint(value: &str, redirect: bool) -> Result<()> {
    let url = reqwest::Url::parse(value)?;
    let loopback = url
        .host_str()
        .is_some_and(|h| matches!(h, "localhost" | "127.0.0.1" | "[::1]"));
    anyhow::ensure!(
        url.scheme() == "https" || (redirect && loopback && url.scheme() == "http"),
        "OAuth endpoints require HTTPS (HTTP loopback redirects are allowed)"
    );
    anyhow::ensure!(
        url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "Invalid OAuth endpoint"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct RejectCallbacks;
    #[async_trait]
    impl OAuthCallbackVerifier<()> for RejectCallbacks {
        async fn consume(&self, _: &str, _: &mut HookContext<Value, ()>) -> Result<String> {
            Err(dog_core::DogError::not_authenticated("unbound callback").into_anyhow())
        }
    }
    #[tokio::test]
    async fn authorization_uses_fresh_state_and_pkce_and_rejects_unbound_exchange() {
        let provider = OAuth2AuthorizationCodeProvider::new(
            OAuth2ClientConfig {
                name: "test".into(),
                client_id: "test".into(),
                client_secret: "test".into(),
                auth_url: "https://example.invalid/authorize".into(),
                token_url: "https://example.invalid/token".into(),
                redirect_uri: "https://app.invalid/callback".into(),
                scopes: vec!["openid".into()],
                userinfo_url: Some("https://example.invalid/userinfo".into()),
            },
            Arc::new(RejectCallbacks),
        )
        .unwrap();
        let first = provider.authorize_url();
        let second = provider.authorize_url();
        assert_ne!(first.state, second.state);
        assert_ne!(first.code_verifier, second.code_verifier);
        let url = reqwest::Url::parse(&first.url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query.get("state").unwrap(), first.state.as_str());
        assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
        let challenge = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(
            first.code_verifier,
        ));
        assert_eq!(query.get("code_challenge").unwrap(), challenge.as_str());
        let app = dog_core::DogAppBuilder::<Value, ()>::new().build();
        let mut ctx = HookContext::new(
            dog_core::TenantContext::new("test"),
            dog_core::ServiceMethodKind::Create,
            (),
            dog_core::ServiceCaller::new(app.clone()),
            app.config_snapshot(),
        );
        assert!(provider
            .exchange_code("code", None, &mut ctx)
            .await
            .is_err());
        assert!(provider
            .exchange_code("code", Some(&first.state), &mut ctx)
            .await
            .is_err());
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn endpoint(response: Vec<u8>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let _ = stream.read(&mut request).await;
            let _ = stream.write_all(&response).await;
        });
        format!("http://{addr}/")
    }
    #[test]
    fn plaintext_provider_endpoints_and_url_credentials_are_rejected() {
        assert!(validate_endpoint("http://provider.example/token", false).is_err());
        assert!(validate_endpoint("http://127.0.0.1/token", false).is_err());
        assert!(validate_endpoint("https://user:secret@provider.example/token", false).is_err());
        assert!(validate_endpoint("https://provider.example/token#fragment", false).is_err());
        assert!(validate_endpoint("http://127.0.0.1/callback", true).is_ok());
        assert!(validate_endpoint("https://provider.example/token", false).is_ok());
    }
    #[tokio::test]
    async fn bounded_http_rejects_declared_and_streamed_oversize_and_truncated_bodies() {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let valid = endpoint(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec()).await;
        let req = oauth2::http::Request::builder()
            .uri(valid)
            .body(vec![])
            .unwrap();
        assert_eq!(
            bounded_request(http.clone(), req).await.unwrap().body(),
            b"{}"
        );
        let mut chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        chunked.extend_from_slice(format!("{:x}\r\n", MAX_PROVIDER_RESPONSE + 1).as_bytes());
        chunked.extend(vec![b'x'; MAX_PROVIDER_RESPONSE + 1]);
        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
        for response in [
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                MAX_PROVIDER_RESPONSE + 1
            )
            .into_bytes(),
            chunked,
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort".to_vec(),
        ] {
            let url = endpoint(response).await;
            let req = oauth2::http::Request::builder()
                .uri(url)
                .body(vec![])
                .unwrap();
            assert!(bounded_request(http.clone(), req).await.is_err());
        }
    }
    struct OnceVerifier(std::sync::atomic::AtomicBool);
    #[async_trait]
    impl OAuthCallbackVerifier<()> for OnceVerifier {
        async fn consume(&self, state: &str, _: &mut HookContext<Value, ()>) -> Result<String> {
            anyhow::ensure!(state == "bound-state", "wrong state");
            anyhow::ensure!(
                !self.0.swap(true, std::sync::atomic::Ordering::SeqCst),
                "replayed"
            );
            Ok("a".repeat(43))
        }
    }
    #[tokio::test]
    async fn token_exchange_consumes_state_and_uses_bounded_http_client() {
        let body = r#"{"access_token":"provider-token","token_type":"bearer"}"#;
        let url = endpoint(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()).await;
        let mut provider = OAuth2AuthorizationCodeProvider::new(
            OAuth2ClientConfig {
                name: "test".into(),
                client_id: "test".into(),
                client_secret: "secret".into(),
                auth_url: "https://provider.invalid/auth".into(),
                token_url: "https://provider.invalid/token".into(),
                redirect_uri: "http://localhost/callback".into(),
                scopes: vec![],
                userinfo_url: None,
            },
            Arc::new(OnceVerifier(std::sync::atomic::AtomicBool::new(false))),
        )
        .unwrap();
        // Test-only endpoint replacement; the public constructor requires HTTPS.
        provider.client = provider.client.set_token_uri(TokenUrl::new(url).unwrap());
        let app = dog_core::DogAppBuilder::<Value, ()>::new().build();
        let mut ctx = HookContext::new(
            dog_core::TenantContext::new("test"),
            dog_core::ServiceMethodKind::Create,
            (),
            dog_core::ServiceCaller::new(app.clone()),
            app.config_snapshot(),
        );
        assert_eq!(
            provider
                .exchange_code("code", Some("bound-state"), &mut ctx)
                .await
                .unwrap(),
            "provider-token"
        );
        assert!(provider
            .exchange_code("code", Some("bound-state"), &mut ctx)
            .await
            .is_err());
    }
}
