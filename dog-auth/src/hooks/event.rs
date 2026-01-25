// Event hook

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::DogAfterHook;
use dog_core::HookContext;
use serde_json::Value;

use crate::core::{AuthenticationResult, ConnectionEvent};
use crate::service::{AuthenticationService, AUTHENTICATION_KEY};

pub trait EventHookParams: Clone + Send + Sync {
    fn provider(&self) -> Option<&str>;
}

impl<P> EventHookParams for super::authenticate::AuthParams<P>
where
    P: Clone + Send + Sync,
{
    fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
}

pub struct EventHook<P>
where
    P: EventHookParams + 'static,
{
    auth_service: Arc<AuthenticationService<P>>,
    event: ConnectionEvent,
}

impl<P> EventHook<P>
where
    P: EventHookParams + 'static,
{
    pub fn new(auth_service: Arc<AuthenticationService<P>>, event: ConnectionEvent) -> Self {
        Self { auth_service, event }
    }

    fn event_name(&self) -> &'static str {
        match self.event {
            ConnectionEvent::Login => "login",
            ConnectionEvent::Logout => "logout",
            ConnectionEvent::Disconnect => "disconnect",
        }
    }
}

#[async_trait]
impl<P> DogAfterHook<Value, P> for EventHook<P>
where
    P: EventHookParams + Clone + Send + Sync + 'static,
{
    async fn run(&self, ctx: &mut HookContext<Value, P>) -> Result<()> {
        let provider = ctx.params.provider().unwrap_or("");
        if provider.trim().is_empty() {
            return Ok(());
        }

        let Some(result) = ctx.result.as_ref() else {
            return Ok(());
        };

        let auth_result: AuthenticationResult = match result {
            dog_core::HookResult::One(v) => v.clone(),
            dog_core::HookResult::Many(vs) => serde_json::to_value(vs).map_err(|e| anyhow::anyhow!(e))?,
        };

        // Emit on the app event hub. This is transport-agnostic; adapters can choose how to publish.
        self.auth_service
            .base
            .app()
            .emit_custom(
                AUTHENTICATION_KEY,
                self.event_name(),
                Arc::new(auth_result),
                ctx,
            )
            .await;

        Ok(())
    }
}