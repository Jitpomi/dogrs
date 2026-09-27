# dog-auth-local

Username/password authentication for DogRS with pluggable entity lookup.

Register `LocalStrategy::new()` on an authentication builder. Use `with_entity_resolver(Arc<dyn LocalEntityResolver<P>>)` for indexed, tenant-scoped lookup in your chosen storage. Alternatively provide `LocalEntityQueryBuilder<P>` to constrain the users service query. The legacy fallback scans the returned service records; it is suitable only for small complete datasets and can miss users in a paginated service.

The resolver must enforce unique usernames within the appropriate tenant and return the stored password hash. Login removes the configured password field, including dotted nested paths, from the entity it returns. Use explicit public projections/protection hooks on other user/JWT/OAuth responses too; arbitrary application secret fields cannot be identified automatically.

## Password behavior

- Passwords must be nonblank and at most **72 UTF-8 bytes**, checked before hashing and verification. Oversized inputs are rejected, never silently truncated.
- bcrypt defaults to cost 10; configuration accepts costs 4–16. Cost 4 is for tests. Benchmark an appropriate production cost for your hardware.
- Hashing and verification run outside async workers, with at most 16 password operations per process. Saturation returns 503 instead of accumulating an unbounded wait queue. A canceled request does not release its permit until blocking work actually completes.
- Missing users, missing hashes, and malformed/out-of-range hashes perform dummy bcrypt verification. This removes the missing-user fast path; it does not promise identical end-to-end response times across database behavior or mixed hash costs. Keep stored costs consistent with configured cost and apply account/IP abuse controls.
- `HashPasswordHook` rejects blank or non-string supplied passwords. An absent field is left unchanged for partial updates.

Passwords already stored using truncation require a migration/reset policy. New logins longer than 72 bytes are rejected, even if their old prefix would previously have matched. Existing hashes above cost 16 are rejected; assess them before upgrading.

The crate does not supply password reset delivery, MFA, an account lockout database, or rate limiting. These are application policies and can use any backend.
