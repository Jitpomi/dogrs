# dog-auth

Transport-independent authentication strategies, hooks, JWT signing, and token lifecycle interfaces for DogRS.

Register strategies on `AuthenticationService::builder(...)` before calling `build()`. Install the service with `AuthenticationService::install(...)`, then initialize the returned adapter with the built application. External adapters must set a trusted provider value and must not accept client-supplied `authenticated` or `auth_result` flags. Authentication identifies a caller; your application still authorizes each resource and tenant.

## JWT

The default features use `jsonwebtoken` with AWS-LC and PEM support. HS256/384/512 and RS256/384/512, ES256/384 are supported. HMAC secrets must contain at least 32 bytes; generate them randomly and keep them out of source control. Signing requires the private PEM and verification requires the public PEM for asymmetric algorithms. `setup_validate()` accepts asymmetric configuration without an HMAC secret.

Verification pins the configured algorithm, issuer, audience, and token type. Access verification rejects refresh tokens. Expiration and `nbf` are enforced with zero clock leeway; synchronize deployment clocks. Tokens require a nonempty `jti` and are limited to 16 KiB. Token lifetimes must be positive and fit the timestamp range. Reserved JWT claims cannot be replaced through `custom_claims`.

Keys are cached per authentication instance. Recreate the instance when replacing keys. Overlapping-key rotation/JWKS discovery is not built in. Applications using a shared issuer/key must enforce tenant authorization; the crate does not infer tenant rights from the request's tenant identifier.

## Revocation and refresh rotation

Configure `AuthenticationBuilder::with_token_store(Arc<dyn TokenStore>)` to enable stateful lifecycle operations without selecting a particular database:

- `is_revoked(issuer, jti)` checks revoked **and consumed** tokens.
- `revoke(issuer, jti, expires_at)` idempotently records a revocation.
- `consume_refresh(issuer, jti, expires_at)` atomically inserts a revocation only if none exists and the token has not expired, returning true for exactly one caller.

Namespace records by issuer and token ID, retain them through expiry, and use shared durable storage across application instances. Implement backend timeouts. Do not evict live records to free capacity: return an error. Verification and rotation fail closed on storage errors. The in-memory store in tests is only a test double.

`rotate_refresh_token(token)` returns a new `TokenPair` and consumes the previous refresh token. Concurrent replay has one winner. If signing fails the old token is not consumed. If the response is lost after consumption, require login again. The crate does not provide token-family compromise detection or undo previously issued access tokens.

`revoke_access_token` and `revoke_refresh_token` revoke individual tokens. Authentication-service `remove` now revokes the supplied access token. To log out a whole session, also revoke its refresh token; application session management must track related tokens. Without a token store, ordinary JWT authentication remains stateless, but logout/revocation and refresh rotation return an error rather than claiming success.

Refresh endpoints must apply current account/permission policy before calling rotation; rotation preserves custom claims from the old token. Account-wide session termination and password-change session policy belong to the application.

See [authentication hardening and migration](../docs/auth-hardening.md) for changes and verification.
