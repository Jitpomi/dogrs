//! # Hooks: Dependency Injection (DogRS style)
//!
//! DogRS is **DI-first**: hooks should be small, portable, testable,
//! and not depend on hidden global state.
//!
//! In FeathersJS, hooks often reach for `context.app` to access
//! config/services. In DogRS, the **default** approach is:
//! **inject what you need at construction time**.
//!
//! However, DogRS also supports an **optional, Feathers-like** runtime access
//! pattern via `ctx.config` and `ctx.services` for cases where DI is awkward.
//!
//! ---
//!
//! ## The two supported styles
//!
//! ### A) Preferred: Dependency Injection (most hooks should do this)
//! ✅ Best for: validation, auth policy checks (if cheap), input shaping,
//! audit stamping, pagination clamping, etc.
//!
//! ```rust
//! use std::sync::Arc;
//! use anyhow::Result;
//! use async_trait::async_trait;
//! use dog_core::{DogBeforeHook, HookContext};
//!
//! struct EnforceMaxPage {
//!     max: usize,
//! }
//!
//! #[async_trait]
//! impl<R, P> DogBeforeHook<R, P> for EnforceMaxPage
//! where
//!     R: Send + 'static,
//!     P: Send + 'static,
//! {
//!     async fn run(&self, _ctx: &mut HookContext<R, P>) -> Result<()> {
//!         // clamp pagination, etc...
//!         Ok(())
//!     }
//! }
//!
//! // Registration:
//! // let max = app.config_snapshot().get_usize("paginate.max").unwrap_or(50);
//! // app.hooks(|h| { h.before_all(Arc::new(EnforceMaxPage { max })); });
//! ```
//!
//! ### B) Optional: Context services/config (Feathers-like escape hatch)
//! ✅ Best for: logging, auditing, light enrichment, or policy checks that
//! genuinely need a separate service and DI is too rigid.
//!
//! DogRS may populate the hook context with:
//! - `ctx.config`: a snapshot of app config at call time
//! - `ctx.services`: a runtime service caller (typed downcast)
//!
//! ```rust
//! use std::sync::Arc;
//! use anyhow::Result;
//! use async_trait::async_trait;
//! use dog_core::{DogBeforeHook, HookContext};
//!
//! // Example types
//! #[derive(Clone)]
//! struct User { id: String }
//! #[derive(Clone)]
//! struct UserParams;
//!
//! struct AttachUser;
//!
//! #[async_trait]
//! impl<Message, Params> DogBeforeHook<Message, Params> for AttachUser
//! where
//!     Message: Send + 'static,
//!     Params: Send + Clone + 'static,
//! {
//!     async fn run(&self, ctx: &mut HookContext<Message, Params>) -> Result<()> {
//!         // Read config snapshot (if provided by the app pipeline):
//!         let _max = ctx.config.get_usize("paginate.max").unwrap_or(50);
//!
//!         // Runtime lookup of another service (typed):
//!         let users = ctx.services.service::<User, UserParams>("users")?;
//!
//!         // NOTE: calling other services from hooks is powerful but risky.
//!         // users.get(...).await?;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ---
//!
//! ## Why `ctx.services` (and not `ctx.service("...")`)?
//!
//! We keep the surface explicit:
//! - `ctx` remains a *pure* per-call context
//! - service lookup is grouped under `ctx.services` so it’s obvious when you’re
//!   reaching outside the hook into the service graph.
//!
//! This mirrors the Feathers mental model (`context.app.service(...)`) without
//! putting the whole `app` onto the hook context.
//!
//! ---
//!
//! ## Important warnings (read this if you use `ctx.services`)
//!
//! Service-to-service calls **inside hooks** can be dangerous because they can:
//! - create hidden coupling (harder to reason about the dependency graph)
//! - accidentally trigger nested hook pipelines (surprising behavior)
//! - form cycles (A hook calls B which triggers a hook that calls A…)
//! - cause performance cliffs (N+1 calls in hooks)
//!
//! Prefer service-to-service calls inside the **service implementation**
//! (domain logic) rather than inside hooks.
//!
//! Use `ctx.services` inside hooks only for:
//! - logging/auditing
//! - lightweight enrichment that cannot live in the service
//! - authorization checks that must query a separate policy service
//!
//! If you do it:
//! - keep it fast and side-effect safe
//! - avoid calling the *same* service you’re currently executing
//! - avoid cascading calls (hook calls service which calls service which…)
//!
//! ---
//!
//! ## Type safety and mismatches
//!
//! `ctx.services.service::<R2, P2>("name")` performs a typed downcast.
//! If you request a different `<R2, P2>` than what was registered,
//! it returns a clear **type mismatch** error.
//!
//! This is deliberate: DogRS remains strongly typed even when providing
//! a Feathers-like runtime lookup experience.
//!



use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::{ServiceMethodKind, TenantContext};

pub enum HookResult<R> {
    One(R),
    Many(Vec<R>),
}

/// A typed, Feathers-inspired hook context.
///
/// This context flows through:
/// around → before → service → after → error
/// A typed, Feathers-inspired hook context.
///
/// This context flows through:
/// around → before → service → after → error
pub struct HookContext<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub tenant: TenantContext,
    pub method: ServiceMethodKind,
    pub params: P,

    /// Input data (create / patch / update)
    pub data: Option<R>,

    /// Output result (after hooks)
    pub result: Option<HookResult<R>>,

    /// Error captured during execution
    pub error: Option<anyhow::Error>,

    /// Feathers-style access to other services (runtime lookup)
    pub services: crate::ServiceCaller<R, P>,

    /// Immutable snapshot of app config for this call
    pub config: crate::DogConfigSnapshot,
}

impl<R, P> HookContext<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub fn new(
        tenant: TenantContext,
        method: ServiceMethodKind,
        params: P,
        services: crate::ServiceCaller<R, P>,
        config: crate::DogConfigSnapshot,
    ) -> Self {
        Self {
            tenant,
            method,
            params,
            data: None,
            result: None,
            error: None,
            services,
            config,
        }
    }
}




pub type HookFut<'a> = Pin<Box<dyn Future<Output = Result<()>> + 'a>>;

/// Around hooks wrap the entire pipeline (like Feathers `around.all`)
pub struct Next<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub(crate) call: Box<dyn for<'a> FnOnce(&'a mut HookContext<R, P>) -> HookFut<'a> + Send>,
}

impl<R, P> Next<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub async fn run<'a>(self, ctx: &'a mut HookContext<R, P>) -> Result<()> {
        (self.call)(ctx).await
    }
}


#[async_trait]
pub trait DogBeforeHook<R, P>: Send + Sync
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()>;
}

#[async_trait]
pub trait DogAfterHook<R, P>: Send + Sync
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()>;
}

#[async_trait]
pub trait DogErrorHook<R, P>: Send + Sync
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()>;
}

#[async_trait]
pub trait DogAroundHook<R, P>: Send + Sync
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    async fn run(&self, ctx: &mut HookContext<R, P>, next: Next<R, P>) -> Result<()>;
}


/// Feathers-style hooks container:
///
/// {
///   around: { all, create, find },
///   before: { all, create },
///   after:  { all, find },
///   error:  { all, create }
/// }
pub struct ServiceHooks<R, P> {
    pub around_all: Vec<Arc<dyn DogAroundHook<R, P>>>,
    pub before_all: Vec<Arc<dyn DogBeforeHook<R, P>>>,
    pub after_all: Vec<Arc<dyn DogAfterHook<R, P>>>,
    pub error_all: Vec<Arc<dyn DogErrorHook<R, P>>>,

    pub around_by_method: HashMap<ServiceMethodKind, Vec<Arc<dyn DogAroundHook<R, P>>>>,
    pub before_by_method: HashMap<ServiceMethodKind, Vec<Arc<dyn DogBeforeHook<R, P>>>>,
    pub after_by_method: HashMap<ServiceMethodKind, Vec<Arc<dyn DogAfterHook<R, P>>>>,
    pub error_by_method: HashMap<ServiceMethodKind, Vec<Arc<dyn DogErrorHook<R, P>>>>,
}

impl<R, P> ServiceHooks<R, P> {
    pub fn new() -> Self {
        Self {
            around_all: Vec::new(),
            before_all: Vec::new(),
            after_all: Vec::new(),
            error_all: Vec::new(),
            around_by_method: HashMap::new(),
            before_by_method: HashMap::new(),
            after_by_method: HashMap::new(),
            error_by_method: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.around_all.is_empty()
            && self.before_all.is_empty()
            && self.after_all.is_empty()
            && self.error_all.is_empty()
            && self.around_by_method.is_empty()
            && self.before_by_method.is_empty()
            && self.after_by_method.is_empty()
            && self.error_by_method.is_empty()
    }

    // ─────────── AROUND ───────────

    pub fn around_all(&mut self, hook: Arc<dyn DogAroundHook<R, P>>) -> &mut Self {
        self.around_all.push(hook);
        self
    }

    pub fn around(
        &mut self,
        method: ServiceMethodKind,
        hook: Arc<dyn DogAroundHook<R, P>>,
    ) -> &mut Self {
        self.around_by_method.entry(method).or_default().push(hook);
        self
    }

    // ─────────── BEFORE ───────────

    pub fn before_all(&mut self, hook: Arc<dyn DogBeforeHook<R, P>>) -> &mut Self {
        self.before_all.push(hook);
        self
    }

    pub fn before(
        &mut self,
        method: ServiceMethodKind,
        hook: Arc<dyn DogBeforeHook<R, P>>,
    ) -> &mut Self {
        self.before_by_method.entry(method).or_default().push(hook);
        self
    }

    pub fn before_create(&mut self, hook: Arc<dyn DogBeforeHook<R, P>>) -> &mut Self {
        self.before(ServiceMethodKind::Create, hook)
    }

    pub fn before_find(&mut self, hook: Arc<dyn DogBeforeHook<R, P>>) -> &mut Self {
        self.before(ServiceMethodKind::Find, hook)
    }

    pub fn before_get(&mut self, hook: Arc<dyn DogBeforeHook<R, P>>) -> &mut Self {
        self.before(ServiceMethodKind::Get, hook)
    }

    pub fn before_update(&mut self, hook: Arc<dyn DogBeforeHook<R, P>>) -> &mut Self {
        self.before(ServiceMethodKind::Update, hook)
    }

    pub fn before_patch(&mut self, hook: Arc<dyn DogBeforeHook<R, P>>) -> &mut Self {
        self.before(ServiceMethodKind::Patch, hook)
    }

    pub fn before_remove(&mut self, hook: Arc<dyn DogBeforeHook<R, P>>) -> &mut Self {
        self.before(ServiceMethodKind::Remove, hook)
    }

    // ─────────── AFTER ───────────

    pub fn after_all(&mut self, hook: Arc<dyn DogAfterHook<R, P>>) -> &mut Self {
        self.after_all.push(hook);
        self
    }

    pub fn after(
        &mut self,
        method: ServiceMethodKind,
        hook: Arc<dyn DogAfterHook<R, P>>,
    ) -> &mut Self {
        self.after_by_method.entry(method).or_default().push(hook);
        self
    }

    pub fn after_create(&mut self, hook: Arc<dyn DogAfterHook<R, P>>) -> &mut Self {
        self.after(ServiceMethodKind::Create, hook)
    }

    pub fn after_find(&mut self, hook: Arc<dyn DogAfterHook<R, P>>) -> &mut Self {
        self.after(ServiceMethodKind::Find, hook)
    }

    // ─────────── ERROR ───────────

    pub fn error_all(&mut self, hook: Arc<dyn DogErrorHook<R, P>>) -> &mut Self {
        self.error_all.push(hook);
        self
    }

    pub fn error(
        &mut self,
        method: ServiceMethodKind,
        hook: Arc<dyn DogErrorHook<R, P>>,
    ) -> &mut Self {
        self.error_by_method.entry(method).or_default().push(hook);
        self
    }
}

/// Helper used by the pipeline:
/// returns `all + method` hooks in that order.
pub(crate) fn collect_method_hooks<T>(
    all: &[T],
    by_method: &std::collections::HashMap<crate::ServiceMethodKind, Vec<T>>,
    method: &crate::ServiceMethodKind,
) -> Vec<T>
where
    T: Clone,
{
    let mut out = Vec::new();
    out.extend_from_slice(all);
    if let Some(v) = by_method.get(method) {
        out.extend_from_slice(v);
    }
    out
}

