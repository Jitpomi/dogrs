use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, DogAfterHook, HookContext};
use serde_json::Value;
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::services::types::AuthDemoParams;

pub struct BeforeRead;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for BeforeRead {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
    
        Ok(())
    }
}

pub struct AfterRead;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for AfterRead {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Query completed
        Ok(())
    }
}

pub struct BeforeWrite;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for BeforeWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {

        Ok(())
    }
}

pub struct AfterWrite;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for AfterWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Write operation completed
        Ok(())
    }
}

pub struct ValidateMessageAuthorExists;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for ValidateMessageAuthorExists {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Validate that message author exists
        Ok(())
    }
}

pub struct ExpandMessageAuthor;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for ExpandMessageAuthor {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Expand message author information
        Ok(())
    }
}

pub struct NormalizeMessagesResult;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for NormalizeMessagesResult {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Normalize messages result format
        Ok(())
    }
}

// Simple Feathers.js-style authenticate function
pub fn authenticate(strategy: &str) -> Arc<dyn DogBeforeHook<Value, AuthDemoParams>> {
    match strategy {
        "jwt" => Arc::new(JwtAuthHook),
        _ => Arc::new(JwtAuthHook), // Default to JWT for now
    }
}

pub struct JwtAuthHook;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for JwtAuthHook {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Extract JWT token from Authorization header
        let auth_header = ctx.params.headers
            .get("authorization")
            .ok_or_else(|| anyhow::anyhow!("Missing Authorization header"))?;

        if !auth_header.starts_with("Bearer ") {
            return Err(anyhow::anyhow!("Invalid Authorization header format"));
        }

        let token = &auth_header[7..]; // Remove "Bearer " prefix

        // Get JWT secret from app context
        let jwt_secret = "demo-secret"; // For now, use hardcoded secret

        // Validate JWT token with signature verification
        let validation = Validation::new(Algorithm::HS256);
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(jwt_secret.as_ref()),
            &validation,
        )
        .map_err(|e| anyhow::anyhow!("Invalid JWT token: {}", e))?;

        let claims = token_data.claims;

        println!("🔐 JWT Authentication successful for user: {}", claims.sub);

        Ok(())
    }
}

// Claims struct for JWT token
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    // Add other claims as needed
}
