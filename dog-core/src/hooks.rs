use anyhow::Result;

use crate::tenant::TenantContext;

/// When in the pipeline a hook is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookStage {
    Around,
    Before,
    After,
    Error,
}

/// Context passed to hooks.
///
/// R = record type
/// P = params type (filters, query options, etc.)
#[derive(Debug)]
pub struct HookContext<R, P> {
    pub tenant: TenantContext,
    pub service_name: &'static str,
    pub method: &'static str, // e.g. "list", "create"
    pub params: P,
    pub record: Option<R>,    // input payload (for create/update)
    pub result: Option<R>,    // output (for after hooks)
    pub error: Option<anyhow::Error>, // error (for error hooks)
}

impl<R, P> HookContext<R, P> {
    pub fn new(
        tenant: TenantContext,
        service_name: &'static str,
        method: &'static str,
        params: P,
    ) -> Self {
        Self {
            tenant,
            service_name,
            method,
            params,
            record: None,
            result: None,
            error: None,
        }
    }
}

/// Core hook trait for DogRS.
///
/// A hook can run `around`, `before`, `after`, or `on error` for a service call.
#[async_trait::async_trait]
pub trait DogHook<R, P>: Send + Sync {
    async fn run(&self, stage: HookStage, ctx: &mut HookContext<R, P>) -> Result<()>;
}
