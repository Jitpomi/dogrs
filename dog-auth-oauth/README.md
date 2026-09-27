# dog-auth-oauth

Provider- and transport-independent OAuth strategy and callback orchestration for DogRS.

Register providers on `OAuthStrategy::new()` or through `OAuthStrategyOptions`, then register the strategy on the authentication builder. `OAuthProvider<P>` implements `exchange_code(code, state, ctx)` and `fetch_profile(access_token, ctx)`. Exactly one code or access token is accepted. Direct token login is disabled by default (including in the built-in provider). Custom providers may opt in through `supports_access_token_login()` only when their profile lookup verifies client/audience binding. Caller-supplied profiles are rejected; a provider-fetched nonempty subject is required. Provider credentials, access tokens, and authorization codes are not included in the authentication result.

## Built-in authorization-code client

Enable `oauth2-client` and construct `OAuth2AuthorizationCodeProvider::new(config, verifier)` with an `OAuth2ClientConfig` and `Arc<dyn OAuthCallbackVerifier<P>>`.

`authorize_url()` returns `OAuthAuthorization { url, state, code_verifier }` with fresh state and S256 PKCE. Store state and verifier server-side, binding them to the initiating browser, tenant, provider and redirect URI. Expire them promptly. The callback verifier must validate that binding and atomically consume the state exactly once before returning its PKCE verifier. Never send the verifier to the browser or accept an unbound state from a client. Multi-instance deployments need shared state storage.

The HTTP client disables redirects, sets a 15-second request deadline, and bounds both token and userinfo responses to 1 MiB while reading, including responses without Content-Length. Endpoints require HTTPS and reject URL credentials/fragments. HTTP loopback callback URLs are allowed for local development; HTTP provider endpoints are not.

This is OAuth authorization-code support, not a complete OpenID Connect ID-token validator. Custom providers must validate their own identity/token semantics. Direct access-token login must only be exposed where the provider/application verifies that the token was issued for the intended client/audience; fetching a profile alone does not establish that binding. Prefer the browser-bound authorization-code flow for browser login.

## Account mapping

If `AuthOptions.entity` is configured, an `OAuthEntityResolver<P>` is **required**. The crate no longer scans a users service then creates a record separately. Implement indexed lookup/atomic upsert with a uniqueness constraint on `(tenant, provider, subject)` in your selected store. A resolver returning `None` rejects login. Return a public user projection without password hashes or other secret fields. Do not link accounts solely by unverified email.

`OAuthService` delegates to the authentication service and an optional `OAuthRedirect<P>`. Redirect destinations must be server-controlled/allowlisted; the generic trait cannot choose your application's destinations. Do not place access tokens in redirect URLs or logs.

The auth demo uses process-local storage for illustration, not a shared production identity database. See [migration and verification](../docs/auth-hardening.md).
