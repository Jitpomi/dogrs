use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use std::sync::Arc;
use bcrypt::{hash, DEFAULT_COST};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

pub struct JwtAuthHook {
    pub jwt_secret: String,
}

impl JwtAuthHook {
    pub fn new(jwt_secret: String) -> Self {
        Self { jwt_secret }
    }
}

#[async_trait]
impl<R, P> DogBeforeHook<R, P> for JwtAuthHook 
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    async fn run(&self, _ctx: &mut HookContext<R, P>) -> Result<()> {
        if self.jwt_secret.len() < 32 {
            return Err(anyhow::anyhow!("JWT secret must be at least 32 characters for security"));
        }
        Ok(())
    }
}

pub fn authenticate<R, P>(strategy: &str) -> Result<Arc<dyn DogBeforeHook<R, P>>, crate::AuthError>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    match strategy {
        "jwt" => {
            let jwt_secret = std::env::var("JWT_SECRET")
                .map_err(|_| crate::AuthError::UnknownStrategy("JWT_SECRET environment variable not set".to_string()))?;
            Ok(Arc::new(JwtAuthHook::new(jwt_secret)))
        },
        _ => Err(crate::AuthError::UnknownStrategy(strategy.to_string())),
    }
}

pub fn hash_password(password: &str) -> Result<String> {
    hash(password, DEFAULT_COST)
        .map_err(|e| anyhow::anyhow!("Password hashing failed: {}", e))
}
