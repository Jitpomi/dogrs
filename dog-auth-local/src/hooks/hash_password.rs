// Hash password hook.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::DogError;
use dog_core::hooks::DogBeforeHook;
use dog_core::HookContext;
use serde_json::Value;

use crate::strategy::LocalStrategy;

pub struct HashPasswordHook<P>
where
    P: Send + Clone + 'static,
{
    pub field: String,
    pub strategy: Arc<LocalStrategy<P>>,
}

impl<P> HashPasswordHook<P>
where
    P: Send + Clone + 'static,
{
    pub fn new(field: impl Into<String>, strategy: Arc<LocalStrategy<P>>) -> Self {
        Self {
            field: field.into(),
            strategy,
        }
    }

    fn get_by_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
        let mut cur = value;
        for part in path.split('.').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            cur = cur.get(part)?;
        }
        Some(cur)
    }

    fn set_by_path(mut value: Value, path: &str, new_value: Value) -> Result<Value> {
        let parts: Vec<&str> = path
            .split('.')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if parts.is_empty() {
            return Ok(value);
        }

        // Only support setting into JSON objects.
        let Some(last) = parts.last().copied() else {
            return Ok(value);
        };

        let mut cur = &mut value;
        for part in &parts[..parts.len() - 1] {
            if !cur.is_object() {
                return Err(DogError::bad_request("Password hash target must be an object").into_anyhow());
            }
            if cur.get(*part).is_none() {
                if let Some(map) = cur.as_object_mut() {
                    map.insert((*part).to_string(), Value::Object(Default::default()));
                }
            }
            cur = cur
                .get_mut(*part)
                .ok_or_else(|| DogError::bad_request("Password hash target path invalid").into_anyhow())?;
        }

        if let Some(map) = cur.as_object_mut() {
            map.insert(last.to_string(), new_value);
            return Ok(value);
        }

        Err(DogError::bad_request("Password hash target must be an object").into_anyhow())
    }

    async fn hash_one(&self, v: Value) -> Result<Value> {
        let Some(pw) = Self::get_by_path(&v, &self.field) else {
            return Ok(v);
        };

        let Some(pw) = pw.as_str() else {
            return Err(DogError::bad_request("Password must be a string").into_anyhow());
        };

        if pw.trim().is_empty() {
            return Ok(v);
        }

        let hashed = self.strategy.hash_password(pw).await?;
        Self::set_by_path(v, &self.field, Value::String(hashed))
    }
}

#[async_trait]
impl<P> DogBeforeHook<Value, P> for HashPasswordHook<P>
where
    P: Send + Clone + 'static,
{
    async fn run(&self, ctx: &mut HookContext<Value, P>) -> Result<()> {
        let Some(data) = ctx.data.take() else {
            return Ok(());
        };

        ctx.data = Some(match data {
            Value::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for v in items {
                    out.push(self.hash_one(v).await?);
                }
                Value::Array(out)
            }
            other => self.hash_one(other).await?,
        });

        Ok(())
    }
}