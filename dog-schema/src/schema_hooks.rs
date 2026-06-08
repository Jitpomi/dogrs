//! # Schema hooks (DogRS-native)
//!
//! Feathers-ish schema utilities:
//! - ResolveData: mutate ctx.data for write methods
//! - ValidateData: validate ctx.data for write methods
//!
//! Key detail: resolvers/validators take `&HookMeta<R,P>` (immutable view)
//! to avoid borrow conflicts with `&mut ctx.data`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use dog_core::{DogBeforeHook, HookContext, ServiceHooks, ServiceMethodKind};

mod private {
    pub trait Sealed {}
}

/// Which write methods should a schema hook apply to?
///
/// # Note on `AllWrites`
/// `AllWrites` covers **Create, Patch, and Update only**. `Remove` is
/// intentionally excluded — it carries no request body to validate or resolve.
/// `Get` and `Find` are excluded because they are read-only operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMethods {
    Create,
    Patch,
    Update,
    /// Matches Create, Patch, and Update. Does **not** include Remove.
    AllWrites,
}

impl WriteMethods {
    #[inline]
    pub fn matches(&self, method: &ServiceMethodKind) -> bool {
        match self {
            WriteMethods::AllWrites => matches!(
                method,
                ServiceMethodKind::Create | ServiceMethodKind::Patch | ServiceMethodKind::Update
            ),
            WriteMethods::Create => matches!(method, ServiceMethodKind::Create),
            WriteMethods::Patch => matches!(method, ServiceMethodKind::Patch),
            WriteMethods::Update => matches!(method, ServiceMethodKind::Update),
        }
    }
}

/// Immutable view of the hook context (safe to pass while mutating ctx.data).
#[derive(Clone)]
pub struct HookMeta<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub tenant: dog_core::TenantContext,
    pub method: dog_core::ServiceMethodKind,
    pub params: P,
    /// Access to other services at validation time.
    ///
    /// # Warning
    /// Calling other services from inside a validator can create hidden coupling,
    /// N+1 query problems, and circular hook pipelines — exactly the anti-patterns
    /// described in `dog-core`'s hook documentation. Prefer expressing cross-entity
    /// constraints inside the service implementation rather than in schema hooks.
    pub services: dog_core::ServiceCaller<R, P>,
    pub config: dog_core::DogConfigSnapshot,
}

impl<R, P> HookMeta<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub(crate) fn from_ctx(ctx: &dog_core::HookContext<R, P>) -> Self {
        Self {
            tenant: ctx.tenant.clone(),
            method: ctx.method.clone(),
            params: ctx.params.clone(),
            services: ctx.services.clone(),
            config: ctx.config.clone(),
        }
    }
}

// ── Shared implementation detail ─────────────────────────────────────────────
//
// `HookBase` holds the `WriteMethods` filter and the method-gating check that
// is identical between `ValidateData` and `ResolveData`.
struct HookBase {
    methods: WriteMethods,
}

impl HookBase {
    fn new(methods: WriteMethods) -> Self {
        Self { methods }
    }

    #[inline]
    fn matches(&self, method: &ServiceMethodKind) -> bool {
        self.methods.matches(method)
    }
}

pub(crate) type ValidateFn<R, P> =
    Arc<dyn Fn(&R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static>;

pub(crate) type ResolveFn<R, P> =
    Arc<dyn Fn(&mut R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static>;

/// Validate `ctx.data` for write methods (create / patch / update).
///
/// The validator closure is **synchronous**. If your validation requires
/// an async operation (e.g. a DB uniqueness check), implement
/// [`dog_core::DogBeforeHook`] directly instead.
pub struct ValidateData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    base: HookBase,
    validator: ValidateFn<R, P>,
}

impl<R, P> ValidateData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub fn new(
        validator: impl Fn(&R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            base: HookBase::new(WriteMethods::AllWrites),
            validator: Arc::new(validator),
        }
    }

    pub fn with_methods(mut self, methods: WriteMethods) -> Self {
        self.base.methods = methods;
        self
    }
}

#[async_trait]
impl<R, P> DogBeforeHook<R, P> for ValidateData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()> {
        if !self.base.matches(&ctx.method) {
            return Ok(());
        }

        let meta = HookMeta::from_ctx(ctx);

        let data = ctx
            .data
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ValidateData requires ctx.data on write methods"))?;

        (self.validator)(data, &meta)
    }
}

/// Resolve/mutate `ctx.data` for write methods (create / patch / update).
///
/// The resolver closure is **synchronous**. If your resolution requires
/// an async operation (e.g. enriching data from the DB), implement
/// [`dog_core::DogBeforeHook`] directly instead.
pub struct ResolveData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    base: HookBase,
    resolver: ResolveFn<R, P>,
}

impl<R, P> ResolveData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub fn new(
        resolver: impl Fn(&mut R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            base: HookBase::new(WriteMethods::AllWrites),
            resolver: Arc::new(resolver),
        }
    }

    pub fn with_methods(mut self, methods: WriteMethods) -> Self {
        self.base.methods = methods;
        self
    }
}

#[async_trait]
impl<R, P> DogBeforeHook<R, P> for ResolveData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()> {
        if !self.base.matches(&ctx.method) {
            return Ok(());
        }

        // capture immutable meta first (no mutable borrow yet)
        let meta = HookMeta::from_ctx(ctx);

        // then mutably borrow data (no ctx immutable borrow needed now)
        let data = ctx
            .data
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("ResolveData requires ctx.data on write methods"))?;

        (self.resolver)(data, &meta)
    }
}

/// Validation rule accumulator — chains field checks and collects all errors before
/// returning them together from [`Rules::check()`].
///
/// # Warning
/// If the chain result is discarded without calling `.check()`, all validation
/// is silently skipped. The `#[must_use]` attribute will warn about this.
#[must_use = "call .check() to propagate validation errors"]
#[derive(Default)]
pub struct Rules {
    errors: Vec<anyhow::Error>,
}

impl Rules {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fails if `v`, after trimming whitespace, is empty.
    pub fn non_empty(mut self, field: &str, v: &str) -> Self {
        if v.trim().is_empty() {
            self.errors
                .push(anyhow::anyhow!("'{field}' must not be empty"));
        }
        self
    }

    /// Fails if `v`, after trimming whitespace, has fewer than `n` characters.
    /// Consistent with [`Self::non_empty`] which also trims before checking.
    pub fn min_len(mut self, field: &str, v: &str, n: usize) -> Self {
        if v.trim().chars().count() < n {
            self.errors
                .push(anyhow::anyhow!("'{field}' must be at least {n} chars"));
        }
        self
    }

    /// Fails if `v`, after trimming whitespace, has more than `n` characters.
    pub fn max_len(mut self, field: &str, v: &str, n: usize) -> Self {
        if v.trim().chars().count() > n {
            self.errors
                .push(anyhow::anyhow!("'{field}' must be at most {n} chars"));
        }
        self
    }

    /// Validates all accumulated rules and returns the combined result.
    ///
    /// Always uses the same error format regardless of how many errors were
    /// collected, so callers can parse the message consistently.
    #[must_use = "discarding the Result silently skips error propagation"]
    pub fn check(self) -> Result<()> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            let msg = self
                .errors
                .iter()
                .map(|e| format!("- {e}"))
                .collect::<Vec<_>>()
                .join("\n");
            Err(anyhow::anyhow!("Schema validation failed:\n{msg}"))
        }
    }
}

/// Fluent builder used by [`SchemaHooksExt::schema()`].
///
/// Obtainable only via the `schema(|s| { ... })` callback — cannot be
/// constructed directly.
pub struct SchemaBuilder<'a, R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    hooks: &'a mut ServiceHooks<R, P>,
    current_methods: WriteMethods,
}

impl<'a, R, P> SchemaBuilder<'a, R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    fn new(hooks: &'a mut ServiceHooks<R, P>) -> Self {
        Self {
            hooks,
            current_methods: WriteMethods::AllWrites,
        }
    }

    pub fn on_create(&mut self) -> &mut Self {
        self.current_methods = WriteMethods::Create;
        self
    }

    pub fn on_patch(&mut self) -> &mut Self {
        self.current_methods = WriteMethods::Patch;
        self
    }

    pub fn on_update(&mut self) -> &mut Self {
        self.current_methods = WriteMethods::Update;
        self
    }

    pub fn on_writes(&mut self) -> &mut Self {
        self.current_methods = WriteMethods::AllWrites;
        self
    }

    pub fn resolve(
        &mut self,
        f: impl Fn(&mut R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static,
    ) -> &mut Self {
        let hook = Arc::new(ResolveData::<R, P>::new(f).with_methods(self.current_methods));
        match self.current_methods {
            WriteMethods::AllWrites => {
                self.hooks.before_all(hook);
            }
            WriteMethods::Create => {
                self.hooks.before_create(hook);
            }
            WriteMethods::Patch => {
                self.hooks.before_patch(hook);
            }
            WriteMethods::Update => {
                self.hooks.before_update(hook);
            }
        }
        self.current_methods = WriteMethods::AllWrites;
        self
    }

    pub fn validate(
        &mut self,
        f: impl Fn(&R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static,
    ) -> &mut Self {
        let hook = Arc::new(ValidateData::<R, P>::new(f).with_methods(self.current_methods));
        match self.current_methods {
            WriteMethods::AllWrites => {
                self.hooks.before_all(hook);
            }
            WriteMethods::Create => {
                self.hooks.before_create(hook);
            }
            WriteMethods::Patch => {
                self.hooks.before_patch(hook);
            }
            WriteMethods::Update => {
                self.hooks.before_update(hook);
            }
        }
        self.current_methods = WriteMethods::AllWrites;
        self
    }
}

/// Extension method: `hooks.schema(|s| ...)`
///
/// # Note
/// This trait is sealed — it is implemented only for [`ServiceHooks`] and
/// cannot be implemented by external crates.
pub trait SchemaHooksExt<R, P>: private::Sealed
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    fn schema<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut SchemaBuilder<'_, R, P>);
}

impl<R, P> private::Sealed for ServiceHooks<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
}

impl<R, P> SchemaHooksExt<R, P> for ServiceHooks<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    fn schema<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut SchemaBuilder<'_, R, P>),
    {
        let mut b = SchemaBuilder::new(self);
        f(&mut b);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dog_core::{DogApp, HookContext, ServiceCaller, ServiceMethodKind, TenantContext};

    // ── Test helpers ───────────────────────────────────────────────────────

    fn make_ctx(method: ServiceMethodKind, data: Option<String>) -> HookContext<String, ()> {
        let app: DogApp<String, ()> = DogApp::default();
        let config = app.config_snapshot();
        let caller = ServiceCaller::new(app);
        let mut ctx = HookContext::new(TenantContext::new("test"), method, (), caller, config);
        ctx.data = data;
        ctx
    }

    // ── Rules ──────────────────────────────────────────────────────────────

    #[test]
    fn rules_passes_when_no_errors() {
        let result = Rules::new()
            .non_empty("name", "Alice")
            .min_len("name", "Alice", 3)
            .check();
        assert!(result.is_ok());
    }

    #[test]
    fn rules_fails_on_empty_field() {
        let err = Rules::new().non_empty("name", "  ").check().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Schema validation failed"));
        assert!(msg.contains("must not be empty"));
    }

    #[test]
    fn rules_fails_on_short_field() {
        let err = Rules::new().min_len("bio", "hi", 5).check().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Schema validation failed"));
        assert!(msg.contains("at least 5 chars"));
    }

    #[test]
    fn rules_aggregates_multiple_errors() {
        let err = Rules::new()
            .non_empty("name", "")
            .min_len("bio", "x", 10)
            .check()
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Schema validation failed"));
        assert!(msg.contains("must not be empty"));
        assert!(msg.contains("at least 10 chars"));
    }

    #[test]
    fn min_len_trims_whitespace_before_counting() {
        let err = Rules::new().min_len("name", "   ", 2).check().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Schema validation failed"));
        assert!(msg.contains("at least 2 chars"));
        assert!(Rules::new().min_len("name", "  hi  ", 2).check().is_ok());
    }

    #[test]
    fn max_len_fails_on_long_field() {
        let err = Rules::new()
            .max_len("bio", "hello world", 5)
            .check()
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Schema validation failed"));
        assert!(msg.contains("at most 5 chars"));
    }

    #[test]
    fn max_len_trims_whitespace_before_counting() {
        // trailing spaces don't count toward the length
        assert!(Rules::new().max_len("bio", "hi   ", 5).check().is_ok());
        // but real chars do
        let err = Rules::new()
            .max_len("bio", "toolongvalue", 5)
            .check()
            .unwrap_err();
        assert!(err.to_string().contains("at most 5 chars"));
    }

    #[test]
    fn max_len_passes_on_exact_length() {
        assert!(Rules::new().max_len("code", "hello", 5).check().is_ok());
    }

    #[test]
    fn write_methods_all_writes_matches_create_patch_update() {
        let wm = WriteMethods::AllWrites;
        assert!(wm.matches(&ServiceMethodKind::Create));
        assert!(wm.matches(&ServiceMethodKind::Patch));
        assert!(wm.matches(&ServiceMethodKind::Update));
        assert!(!wm.matches(&ServiceMethodKind::Find));
        assert!(!wm.matches(&ServiceMethodKind::Get));
        assert!(!wm.matches(&ServiceMethodKind::Remove));
    }

    #[test]
    fn write_methods_create_only_matches_create() {
        let wm = WriteMethods::Create;
        assert!(wm.matches(&ServiceMethodKind::Create));
        assert!(!wm.matches(&ServiceMethodKind::Patch));
        assert!(!wm.matches(&ServiceMethodKind::Update));
    }

    #[test]
    fn write_methods_patch_only_matches_patch() {
        assert!(WriteMethods::Patch.matches(&ServiceMethodKind::Patch));
        assert!(!WriteMethods::Patch.matches(&ServiceMethodKind::Create));
    }

    #[test]
    fn write_methods_update_only_matches_update() {
        assert!(WriteMethods::Update.matches(&ServiceMethodKind::Update));
        assert!(!WriteMethods::Update.matches(&ServiceMethodKind::Create));
    }

    // ── ValidateData ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn validate_data_fires_on_matching_method() {
        let hook = ValidateData::<String, ()>::new(|data, _meta| {
            if data.is_empty() {
                anyhow::bail!("must not be empty");
            }
            Ok(())
        });
        let mut ctx = make_ctx(ServiceMethodKind::Create, Some("hello".to_string()));
        assert!(hook.run(&mut ctx).await.is_ok());
    }

    #[tokio::test]
    async fn validate_data_propagates_error() {
        let hook = ValidateData::<String, ()>::new(|_, _| anyhow::bail!("validation failed"));
        let mut ctx = make_ctx(ServiceMethodKind::Create, Some("any".to_string()));
        let err = hook.run(&mut ctx).await.unwrap_err();
        assert!(err.to_string().contains("validation failed"));
    }

    #[tokio::test]
    async fn validate_data_skips_on_non_matching_method() {
        let hook = ValidateData::<String, ()>::new(|_, _| anyhow::bail!("should not run"))
            .with_methods(WriteMethods::Create);
        let mut ctx = make_ctx(ServiceMethodKind::Find, None);
        assert!(hook.run(&mut ctx).await.is_ok());
    }

    #[tokio::test]
    async fn validate_data_errors_on_missing_data() {
        let hook = ValidateData::<String, ()>::new(|_, _| Ok(()));
        let mut ctx = make_ctx(ServiceMethodKind::Create, None);
        let err = hook.run(&mut ctx).await.unwrap_err();
        assert!(err.to_string().contains("ValidateData requires ctx.data"));
    }

    // ── ResolveData ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn resolve_data_mutates_ctx_data() {
        let hook = ResolveData::<String, ()>::new(|data, _meta| {
            *data = format!("resolved:{data}");
            Ok(())
        });
        let mut ctx = make_ctx(ServiceMethodKind::Create, Some("raw".to_string()));
        hook.run(&mut ctx).await.unwrap();
        assert_eq!(ctx.data.as_deref(), Some("resolved:raw"));
    }

    #[tokio::test]
    async fn resolve_data_skips_on_non_matching_method() {
        let hook = ResolveData::<String, ()>::new(|data, _| {
            *data = "mutated".to_string();
            Ok(())
        })
        .with_methods(WriteMethods::Create);
        let mut ctx = make_ctx(ServiceMethodKind::Find, None);
        assert!(hook.run(&mut ctx).await.is_ok());
        assert!(ctx.data.is_none()); // data untouched
    }

    #[tokio::test]
    async fn resolve_data_errors_on_missing_data() {
        let hook = ResolveData::<String, ()>::new(|_, _| Ok(()));
        let mut ctx = make_ctx(ServiceMethodKind::Create, None);
        let err = hook.run(&mut ctx).await.unwrap_err();
        assert!(err.to_string().contains("ResolveData requires ctx.data"));
    }

    // ── SchemaBuilder / SchemaHooksExt ─────────────────────────────────────

    #[tokio::test]
    async fn schema_builder_registers_hook_and_fires_on_correct_method() {
        use std::sync::Mutex;

        let called = Arc::new(Mutex::new(false));
        let called_clone = Arc::clone(&called);

        let mut hooks: ServiceHooks<String, ()> = ServiceHooks::new();
        hooks.schema(|s| {
            s.on_create().validate(move |_, _| {
                *called_clone.lock().unwrap() = true;
                Ok(())
            });
        });

        // Hook goes to before_create bucket, NOT before_all
        assert_eq!(
            hooks.before_all.len(),
            0,
            "expected hook in before_create, not before_all"
        );
        let create_hooks = hooks
            .before_by_method
            .get(&ServiceMethodKind::Create)
            .expect("no hooks in before_create bucket");
        assert_eq!(create_hooks.len(), 1);

        // Hook fires on Create and marks the flag
        let mut ctx = make_ctx(ServiceMethodKind::Create, Some("test".to_string()));
        create_hooks[0].run(&mut ctx).await.unwrap();
        assert!(
            *called.lock().unwrap(),
            "validator was not called on Create"
        );
    }

    #[tokio::test]
    async fn schema_builder_on_create_does_not_fire_on_patch() {
        let mut hooks: ServiceHooks<String, ()> = ServiceHooks::new();
        hooks.schema(|s| {
            s.on_create()
                .validate(|_, _| anyhow::bail!("should not fire on Patch"));
        });

        // Hook is in before_create, not before_all or before_patch
        assert_eq!(hooks.before_all.len(), 0);
        assert!(
            hooks
                .before_by_method
                .get(&ServiceMethodKind::Patch)
                .map_or(true, |v| v.is_empty()),
            "hook must not appear in before_patch bucket"
        );
    }

    #[tokio::test]
    async fn schema_builder_resets_methods_between_calls() {
        // After on_create().validate(), the next validate() should scope to AllWrites
        let mut hooks: ServiceHooks<String, ()> = ServiceHooks::new();
        hooks.schema(|s| {
            s.on_create().validate(|_, _| Ok(())); // Create bucket, resets
            s.validate(|_, _| anyhow::bail!("all-writes hook")); // AllWrites → before_all
        });

        // Create-scoped hook in before_create bucket
        let create_hooks = hooks
            .before_by_method
            .get(&ServiceMethodKind::Create)
            .expect("no hooks in before_create");
        assert_eq!(create_hooks.len(), 1);

        // All-writes hook in before_all
        assert_eq!(hooks.before_all.len(), 1);

        // before_all hook fires on Patch
        let mut ctx = make_ctx(ServiceMethodKind::Patch, Some("data".to_string()));
        let err = hooks.before_all[0].run(&mut ctx).await.unwrap_err();
        assert!(err.to_string().contains("all-writes hook"));

        // Create-only hook fires on Create
        let mut ctx2 = make_ctx(ServiceMethodKind::Create, Some("data".to_string()));
        assert!(create_hooks[0].run(&mut ctx2).await.is_ok());
    }
}
