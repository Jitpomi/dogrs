// Connection hook.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::DogAfterHook;
use dog_core::HookContext;
use serde_json::Value;

use crate::core::{AuthenticationResult, ConnectionEvent};
use crate::service::AuthenticationService;

pub trait ConnectionHookParams: Clone + Send + Sync {
    fn connection(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>>;
}

impl<P> ConnectionHookParams for super::authenticate::AuthParams<P>
where
    P: Clone + Send + Sync,
{
    fn connection(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.connection.clone()
    }
}

pub struct ConnectionHook<P>
where
    P: ConnectionHookParams + 'static,
{
    auth_service: Arc<AuthenticationService<P>>,
    event: ConnectionEvent,
}

impl<P> ConnectionHook<P>
where
    P: ConnectionHookParams + 'static,
{
    pub fn new(auth_service: Arc<AuthenticationService<P>>, event: ConnectionEvent) -> Self {
        Self { auth_service, event }
    }
}

#[async_trait]
impl<P> DogAfterHook<Value, P> for ConnectionHook<P>
where
    P: ConnectionHookParams + Clone + Send + Sync + 'static,
{
    async fn run(&self, ctx: &mut HookContext<Value, P>) -> Result<()> {
        let Some(connection) = ctx.params.connection() else {
            return Ok(());
        };

        let Some(result) = ctx.result.as_ref() else {
            return Ok(());
        };

        let auth_result: AuthenticationResult = match result {
            dog_core::HookResult::One(v) => v.clone(),
            dog_core::HookResult::Many(vs) => serde_json::to_value(vs).map_err(|e| anyhow::anyhow!(e))?,
        };

        self.auth_service
            .handle_connection(self.event, connection, &auth_result)
            .await?;

        Ok(())
    }
}