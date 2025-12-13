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
pub struct HookContext<R, P> {
    pub tenant: TenantContext,
    pub method: ServiceMethodKind,
    pub params: P,

    /// Input data (create / patch / update)
    pub data: Option<R>,

    /// Output result (after hooks)
    pub result: Option<HookResult<R>>,

    /// Error captured during execution
    pub error: Option<anyhow::Error>,
}

impl<R, P> HookContext<R, P> {
    pub fn new(tenant: TenantContext, method: ServiceMethodKind, params: P) -> Self {
        Self {
            tenant,
            method,
            params,
            data: None,
            result: None,
            error: None,
        }
    }
}

#[async_trait]
pub trait DogBeforeHook<R, P>: Send + Sync {
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()>;
}

#[async_trait]
pub trait DogAfterHook<R, P>: Send + Sync {
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()>;
}

#[async_trait]
pub trait DogErrorHook<R, P>: Send + Sync {
    async fn run(&self, ctx: &mut HookContext<R, P>) -> Result<()>;
}

pub type HookFut<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// Around hooks wrap the entire pipeline (like Feathers `around.all`)
pub struct Next<R, P> {
    pub(crate) call: Box<dyn for<'a> FnOnce(&'a mut HookContext<R, P>) -> HookFut<'a> + Send>,
}

impl<R, P> Next<R, P> {
    pub async fn run<'a>(self, ctx: &'a mut HookContext<R, P>) -> Result<()> {
        (self.call)(ctx).await
    }
}

#[async_trait]
pub trait DogAroundHook<R, P>: Send + Sync {
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

