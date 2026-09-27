# Authentication hardening

This change hardens `dog-auth`, `dog-auth-local`, and `dog-auth-oauth` without requiring a database, cache, or HTTP server.

## Upgrade requirements

1. Generate a random HMAC secret of at least 32 bytes, or configure asymmetric PEM keys. The auth demo no longer has a known default secret. Set `AUTH_JWT_SECRET` explicitly.
2. Configure a `TokenStore` for logout/revocation or refresh rotation. Stateless token verification remains available. Authentication-service `remove` now errors without revocation storage; it no longer treats verification as logout. Revoking an access token alone does not revoke its sibling refresh token.
3. JWT verification now requires issuer, audience, expiration and token ID, enforces `nbf`, and uses zero clock leeway. Reissue incompatible old tokens. Reserved claims cannot be set through `custom_claims`. Keep deployment clocks synchronized.
4. Passwords longer than 72 UTF-8 bytes and blank passwords are rejected. Assess old bcrypt hashes/password policies before rollout. Costs outside 4–16 are rejected. Password-worker saturation returns 503.
5. Configure an atomic `OAuthEntityResolver` when attaching a local entity. The implicit full-service scan/create fallback is removed. The storage implementation must enforce uniqueness across instances.
6. Direct OAuth access-token login now requires explicit provider opt-in with client/audience validation. The built-in provider accepts the code/state flow only.
7. Built-in OAuth provider endpoints require HTTPS and responses are capped at 1 MiB. Only loopback callback redirects may use HTTP.

## Verification

Regression coverage includes concurrent refresh replay, revocation and storage failure, invalid JWT claims/lifetimes, RSA/ECDSA setup, bcrypt byte limits, missing-user dummy work, password admission saturation, mandatory OAuth entity resolution, one-use callback state, and real loopback HTTP token exchange and oversized/truncated provider responses. These tests run in CI with the optional OAuth client enabled.

Application responsibilities remain: resource/tenant authorization, abuse controls, MFA/password resets if required, secure cookies and TLS, atomic durable storage implementations, OAuth client/audience validation and browser state binding, redirect allowlists, current-account checks at refresh time, and public user projections. The framework does not claim to certify an arbitrary supplied store, provider, or resolver.
