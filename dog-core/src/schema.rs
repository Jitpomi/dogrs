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

use crate::{DogBeforeHook, HookContext, ServiceHooks, ServiceMethodKind};

/// Which write methods should a schema hook apply to?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMethods {
    Create,
    Patch,
    Update,
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
    pub tenant: crate::TenantContext,
    pub method: crate::ServiceMethodKind,
    pub params: P,
    pub services: crate::ServiceCaller<R, P>,
    pub config: crate::DogConfigSnapshot,
}

impl<R, P> HookMeta<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub fn from_ctx(ctx: &crate::HookContext<R, P>) -> Self {
        Self {
            tenant: ctx.tenant.clone(),
            method: ctx.method.clone(),
            params: ctx.params.clone(),
            services: ctx.services.clone(),
            config: ctx.config.clone(),
        }
    }
}

pub type ValidateFn<R, P> =
    Arc<dyn Fn(&R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static>;

pub type ResolveFn<R, P> =
    Arc<dyn Fn(&mut R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static>;

/// Validate `ctx.data` for create/patch/update. (Feathers `validateData`)
pub struct ValidateData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    methods: WriteMethods,
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
            methods: WriteMethods::AllWrites,
            validator: Arc::new(validator),
        }
    }

    pub fn with_methods(mut self, methods: WriteMethods) -> Self {
        self.methods = methods;
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
        if !self.methods.matches(&ctx.method) {
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

/// Resolve/mutate `ctx.data` for create/patch/update. (Feathers `resolveData`)
pub struct ResolveData<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    methods: WriteMethods,
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
            methods: WriteMethods::AllWrites,
            resolver: Arc::new(resolver),
        }
    }

    pub fn with_methods(mut self, methods: WriteMethods) -> Self {
        self.methods = methods;
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
        if !self.methods.matches(&ctx.method) {
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

/// Tiny “rules” helper for nicer validation errors.
#[derive(Default)]
pub struct Rules {
    errors: Vec<anyhow::Error>,
}

impl Rules {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn non_empty(mut self, field: &str, v: &str) -> Self {
        if v.trim().is_empty() {
            self.errors
                .push(anyhow::anyhow!("'{field}' must not be empty"));
        }
        self
    }

    pub fn min_len(mut self, field: &str, v: &str, n: usize) -> Self {
        if v.chars().count() < n {
            self.errors
                .push(anyhow::anyhow!("'{field}' must be at least {n} chars"));
        }
        self
    }

    pub fn check(self) -> Result<()> {
        if self.errors.is_empty() {
            Ok(())
        } else if self.errors.len() == 1 {
            Err(self.errors.into_iter().next().unwrap())
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

/// Fluent builder used by `ServiceHooks::schema(...)`.
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
        let hook = ResolveData::<R, P>::new(f).with_methods(self.current_methods);
        self.hooks.before_all(Arc::new(hook));
        self
    }

    pub fn validate(
        &mut self,
        f: impl Fn(&R, &HookMeta<R, P>) -> Result<()> + Send + Sync + 'static,
    ) -> &mut Self {
        let hook = ValidateData::<R, P>::new(f).with_methods(self.current_methods);
        self.hooks.before_all(Arc::new(hook));
        self
    }
}

/// Extension method: `hooks.schema(|s| ...)`
pub trait SchemaHooksExt<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    fn schema<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut SchemaBuilder<'_, R, P>);
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
