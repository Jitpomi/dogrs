use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;

use crate::hooks::{collect_method_hooks, HookFut};
use crate::{
    DogConfig, DogService, DogServiceRegistry, HookContext, HookResult, Next, ServiceHooks,
    ServiceMethodKind, TenantContext,
};

struct DogAppInner<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    registry: RwLock<DogServiceRegistry<R, P>>,
    global_hooks: RwLock<ServiceHooks<R, P>>,
    service_hooks: RwLock<HashMap<String, ServiceHooks<R, P>>>,
    config: RwLock<DogConfig>,

    // Store the concrete: Arc<dyn DogService<R,P>> as Box<dyn Any>
    any_services: RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>,
}

/// DogApp is the central application container for DogRS.
///
/// Framework-agnostic. Holds:
/// - service registry
/// - app hooks
/// - per-service hooks
/// - config
pub struct DogApp<R, P = ()>
where
    R: Send + 'static,
    P: Send + 'static,
{
    inner: Arc<DogAppInner<R, P>>,
}

impl<R, P> Clone for DogApp<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<R, P> DogApp<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DogAppInner {
                registry: RwLock::new(DogServiceRegistry::new()),
                global_hooks: RwLock::new(ServiceHooks::new()),
                service_hooks: RwLock::new(HashMap::new()),
                config: RwLock::new(DogConfig::new()),
                any_services: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub fn register_service<S>(&self, name: S, service: Arc<dyn DogService<R, P>>)
    where
        S: Into<String>,
    {
        let name = name.into();

        // typed registry
        self.inner
            .registry
            .write()
            .unwrap()
            .register(name.clone(), service.clone());

        // any registry: store the concrete Arc<dyn DogService<R,P>>
        self.inner
            .any_services
            .write()
            .unwrap()
            .insert(name, Box::new(service));
    }

    /// Feathers: `app.hooks({ ... })`
    pub fn hooks<F>(&self, f: F)
    where
        F: FnOnce(&mut ServiceHooks<R, P>),
    {
        let mut g = self.inner.global_hooks.write().unwrap();
        f(&mut g);
    }

    /// Feathers: `app.service("x").hooks({ ... })`
    pub(crate) fn configure_service_hooks<F>(&self, service_name: &str, f: F)
    where
        F: FnOnce(&mut ServiceHooks<R, P>),
    {
        let mut map = self.inner.service_hooks.write().unwrap();
        let hooks = map
            .entry(service_name.to_string())
            .or_insert_with(ServiceHooks::new);
        f(hooks);
    }

    /// Feathers: `app.service("name")`
    pub fn service(&self, name: &str) -> Result<ServiceHandle<R, P>> {
        let svc = self
            .inner
            .registry
            .read()
            .unwrap()
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("DogService not found: {name}"))?
            .clone();

        Ok(ServiceHandle {
            app: self.clone(),
            name: name.to_string(),
            service: svc,
        })
    }

    /// Feathers: `app.set(key, value)`
    pub fn set<K, V>(&self, key: K, value: V)
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.inner.config.write().unwrap().set(key, value);
    }

    /// Feathers: `app.get(key)`
    pub fn get(&self, key: &str) -> Option<String> {
        let cfg = self.inner.config.read().unwrap();
        cfg.get(key).map(|v| v.to_string())
    }

    pub fn config_snapshot(&self) -> crate::DogConfigSnapshot {
        let cfg = self.inner.config.read().unwrap();
        cfg.snapshot()
    }
}

pub struct ServiceHandle<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    app: DogApp<R, P>,
    name: String,
    service: Arc<dyn DogService<R, P>>,
}

impl<R, P> ServiceHandle<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    pub fn hooks<F>(self, f: F) -> Self
    where
        F: FnOnce(&mut ServiceHooks<R, P>),
    {
        self.app.configure_service_hooks(&self.name, f);
        self
    }

    pub fn inner(&self) -> &Arc<dyn DogService<R, P>> {
        &self.service
    }
}

// ──────────────────────────────────────────────────────────────
// Pipeline helper (extracted)
// ──────────────────────────────────────────────────────────────

impl<R, P> ServiceHandle<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    /// Collect hooks in Feathers order:
    /// global first, then service.
    fn collect_hooks_for_method(
        &self,
        method: &ServiceMethodKind,
    ) -> (
        Vec<Arc<dyn crate::DogAroundHook<R, P>>>,
        Vec<Arc<dyn crate::DogBeforeHook<R, P>>>,
        Vec<Arc<dyn crate::DogAfterHook<R, P>>>,
        Vec<Arc<dyn crate::DogErrorHook<R, P>>>,
    ) {
        let g = self.app.inner.global_hooks.read().unwrap();
        let map = self.app.inner.service_hooks.read().unwrap();
        let s = map.get(&self.name);

        // GLOBAL
        let mut around = collect_method_hooks(&g.around_all, &g.around_by_method, method);
        let mut before = collect_method_hooks(&g.before_all, &g.before_by_method, method);
        let mut after = collect_method_hooks(&g.after_all, &g.after_by_method, method);
        let mut error = collect_method_hooks(&g.error_all, &g.error_by_method, method);

        // SERVICE (append after global)
        if let Some(h) = s {
            around.extend(collect_method_hooks(
                &h.around_all,
                &h.around_by_method,
                method,
            ));
            before.extend(collect_method_hooks(
                &h.before_all,
                &h.before_by_method,
                method,
            ));
            after.extend(collect_method_hooks(
                &h.after_all,
                &h.after_by_method,
                method,
            ));
            error.extend(collect_method_hooks(
                &h.error_all,
                &h.error_by_method,
                method,
            ));
        }

        (around, before, after, error)
    }

    /// Core Feathers pipeline:
    /// around → before → service_call → after → error
    async fn run_pipeline(
        &self,
        method: ServiceMethodKind,
        mut ctx: HookContext<R, P>,
        service_call: Arc<
            dyn for<'a> Fn(Arc<dyn DogService<R, P>>, &'a mut HookContext<R, P>) -> HookFut<'a>
                + Send
                + Sync,
        >,
    ) -> Result<HookContext<R, P>> {
        let (around, before, after, error) = self.collect_hooks_for_method(&method);

        let svc = self.service.clone();
        let service_call_inner = service_call.clone();

        // Inner: BEFORE -> service_call -> AFTER
        let mut next: Next<R, P> = Next {
            call: Box::new(move |ctx: &mut HookContext<R, P>| -> HookFut<'_> {
                let before = before.clone();
                let after = after.clone();
                let svc = svc.clone();
                let service_call = service_call_inner.clone();

                Box::pin(async move {
                    for h in &before {
                        h.run(ctx).await?;
                    }

                    // sets ctx.result
                    (service_call)(svc, ctx).await?;

                    for h in after.iter().rev() {
                        h.run(ctx).await?;
                    }

                    Ok(())
                })
            }),
        };

        // AROUND chain: first hook is outermost
        for h in around.iter().rev() {
            let hook = h.clone();
            let prev = next;
            next = Next {
                call: Box::new(move |ctx: &mut HookContext<R, P>| -> HookFut<'_> {
                    let hook = hook.clone();
                    Box::pin(async move { hook.run(ctx, prev).await })
                }),
            };
        }

        // Execute and apply ERROR hooks if needed
        let res = next.run(&mut ctx).await;

        if let Err(e) = res {
            ctx.error = Some(e);

            for h in &error {
                let _ = h.run(&mut ctx).await;
            }

            if let Some(err) = ctx.error.take() {
                return Err(err);
            }
        }

        Ok(ctx)
    }

    // ──────────────────────────────────────────────────────────────
    // Methods wired through helper
    // ──────────────────────────────────────────────────────────────

    pub async fn find(&self, tenant: TenantContext, params: P) -> Result<Vec<R>> {
        let method = ServiceMethodKind::Find;

        let services = ServiceCaller::new(self.app.clone());
        let config = self.app.config_snapshot();
        let ctx = HookContext::new(tenant, method.clone(), params, services, config);

        let ctx = self
            .run_pipeline(
                method,
                ctx,
                Arc::new(|svc, ctx| {
                    Box::pin(async move {
                        let records = svc.find(&ctx.tenant, ctx.params.clone()).await?;
                        ctx.result = Some(HookResult::Many(records));
                        Ok(())
                    })
                }),
            )
            .await?;

        match ctx.result {
            Some(HookResult::Many(v)) => Ok(v),
            Some(HookResult::One(_)) => Err(anyhow::anyhow!(
                "find() produced HookResult::One unexpectedly"
            )),
            None => Ok(vec![]),
        }
    }

    pub async fn get(&self, tenant: TenantContext, id: &str, params: P) -> Result<R> {
        let method = ServiceMethodKind::Get;

        let services = ServiceCaller::new(self.app.clone());
        let config = self.app.config_snapshot();
        let ctx = HookContext::new(tenant, method.clone(), params, services, config);

        let id = id.to_string();

        let ctx = self
            .run_pipeline(
                method,
                ctx,
                Arc::new(move |svc, ctx| {
                    let id = id.clone();
                    Box::pin(async move {
                        let record = svc.get(&ctx.tenant, &id, ctx.params.clone()).await?;
                        ctx.result = Some(HookResult::One(record));
                        Ok(())
                    })
                }),
            )
            .await?;

        match ctx.result {
            Some(HookResult::One(v)) => Ok(v),
            Some(HookResult::Many(_)) => Err(anyhow::anyhow!(
                "get() produced HookResult::Many unexpectedly"
            )),
            None => Err(anyhow::anyhow!("get() produced no result")),
        }
    }

    pub async fn create(&self, tenant: TenantContext, data: R, params: P) -> Result<R> {
        let method = ServiceMethodKind::Create;

        let services = ServiceCaller::new(self.app.clone());
        let config = self.app.config_snapshot();
        let mut ctx = HookContext::new(tenant, method.clone(), params, services, config);
        ctx.data = Some(data);

        let ctx = self
            .run_pipeline(
                method,
                ctx,
                Arc::new(|svc, ctx| {
                    Box::pin(async move {
                        let data = ctx
                            .data
                            .take()
                            .ok_or_else(|| anyhow::anyhow!("create() requires ctx.data"))?;

                        let created = svc.create(&ctx.tenant, data, ctx.params.clone()).await?;
                        ctx.result = Some(HookResult::One(created));
                        Ok(())
                    })
                }),
            )
            .await?;

        match ctx.result {
            Some(HookResult::One(v)) => Ok(v),
            Some(HookResult::Many(_)) => Err(anyhow::anyhow!(
                "create() produced HookResult::Many unexpectedly"
            )),
            None => Err(anyhow::anyhow!("create() produced no result")),
        }
    }

    pub async fn patch(
        &self,
        tenant: TenantContext,
        id: Option<&str>,
        data: R,
        params: P,
    ) -> Result<R> {
        let method = ServiceMethodKind::Patch;

        let services = ServiceCaller::new(self.app.clone());
        let config = self.app.config_snapshot();
        let mut ctx = HookContext::new(tenant, method.clone(), params, services, config);
        ctx.data = Some(data);

        let id: Option<String> = id.map(|s| s.to_string());

        let ctx = self
            .run_pipeline(
                method,
                ctx,
                Arc::new(move |svc, ctx| {
                    let id = id.clone();
                    Box::pin(async move {
                        let data = ctx
                            .data
                            .take()
                            .ok_or_else(|| anyhow::anyhow!("patch() requires ctx.data"))?;

                        let patched = svc
                            .patch(&ctx.tenant, id.as_deref(), data, ctx.params.clone())
                            .await?;

                        ctx.result = Some(HookResult::One(patched));
                        Ok(())
                    })
                }),
            )
            .await?;

        match ctx.result {
            Some(HookResult::One(v)) => Ok(v),
            Some(HookResult::Many(_)) => Err(anyhow::anyhow!(
                "patch() produced HookResult::Many unexpectedly"
            )),
            None => Err(anyhow::anyhow!("patch() produced no result")),
        }
    }

    pub async fn update(&self, tenant: TenantContext, id: &str, data: R, params: P) -> Result<R> {
        let method = ServiceMethodKind::Update;

        let services = ServiceCaller::new(self.app.clone());
        let config = self.app.config_snapshot();
        let mut ctx = HookContext::new(tenant, method.clone(), params, services, config);
        ctx.data = Some(data);

        let id = id.to_string();

        let ctx = self
            .run_pipeline(
                method,
                ctx,
                Arc::new(move |svc, ctx| {
                    let id = id.clone();
                    Box::pin(async move {
                        let data = ctx
                            .data
                            .take()
                            .ok_or_else(|| anyhow::anyhow!("update() requires ctx.data"))?;

                        let updated = svc
                            .update(&ctx.tenant, &id, data, ctx.params.clone())
                            .await?;

                        ctx.result = Some(HookResult::One(updated));
                        Ok(())
                    })
                }),
            )
            .await?;

        match ctx.result {
            Some(HookResult::One(v)) => Ok(v),
            Some(HookResult::Many(_)) => Err(anyhow::anyhow!(
                "update() produced HookResult::Many unexpectedly"
            )),
            None => Err(anyhow::anyhow!("update() produced no result")),
        }
    }

    pub async fn remove(&self, tenant: TenantContext, id: Option<&str>, params: P) -> Result<R> {
        let method = ServiceMethodKind::Remove;

        let services = ServiceCaller::new(self.app.clone());
        let config = self.app.config_snapshot();
        let ctx = HookContext::new(tenant, method.clone(), params, services, config);

        let id: Option<String> = id.map(|s| s.to_string());

        let ctx = self
            .run_pipeline(
                method,
                ctx,
                Arc::new(move |svc, ctx| {
                    let id = id.clone();
                    Box::pin(async move {
                        let removed = svc
                            .remove(&ctx.tenant, id.as_deref(), ctx.params.clone())
                            .await?;

                        ctx.result = Some(HookResult::One(removed));
                        Ok(())
                    })
                }),
            )
            .await?;

        match ctx.result {
            Some(HookResult::One(v)) => Ok(v),
            Some(HookResult::Many(_)) => Err(anyhow::anyhow!(
                "remove() produced HookResult::Many unexpectedly"
            )),
            None => Err(anyhow::anyhow!("remove() produced no result")),
        }
    }
}

#[derive(Clone)]
pub struct ServiceCaller<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    app: DogApp<R, P>,
}

impl<R, P> ServiceCaller<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub fn new(app: DogApp<R, P>) -> Self {
        Self { app }
    }

    /// Feathers-style runtime service lookup:
    /// ctx.services.service::<User, UserParams>("users")?
    pub fn service<R2, P2>(&self, name: &str) -> Result<Arc<dyn DogService<R2, P2>>>
    where
        R2: Send + 'static,
        P2: Send + Clone + 'static,
    {
        let map = self.app.inner.any_services.read().unwrap();

        let any = map
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("DogService not found: {name}"))?;

        let stored = any
            .downcast_ref::<Arc<dyn DogService<R2, P2>>>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "DogService type mismatch for '{name}'. You requested a different <R,P> than what was registered."
                )
            })?;

        Ok(stored.clone())
    }
}
