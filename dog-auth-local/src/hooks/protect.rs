// Protect hook.

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::DogAfterHook;
use dog_core::{HookContext, HookResult};
use serde_json::Value;

pub trait ProtectHookParams: Clone + Send + Sync {
    fn provider(&self) -> Option<&str>;
}

impl<P> ProtectHookParams for dog_auth::hooks::authenticate::AuthParams<P>
where
    P: Clone + Send + Sync,
{
    fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
}

pub struct ProtectHook<P>
where
    P: ProtectHookParams + 'static,
{
    fields: Vec<String>,
    _phantom: std::marker::PhantomData<P>,
}

impl<P> ProtectHook<P>
where
    P: ProtectHookParams + 'static,
{
    pub fn new(fields: Vec<String>) -> Self {
        Self {
            fields,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn from_fields(fields: &[&str]) -> Self {
        Self::new(fields.iter().map(|s| s.to_string()).collect())
    }

    fn strip_one(&self, mut v: Value) -> Value {
        for f in &self.fields {
            Self::remove_path(&mut v, f);
        }
        v
    }

    fn remove_path(root: &mut Value, path: &str) {
        let parts: Vec<&str> = path
            .split('.')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return;
        }

        let Some(last) = parts.last().copied() else {
            return;
        };

        let mut cur = root;
        for p in &parts[..parts.len() - 1] {
            match cur {
                Value::Object(map) => {
                    let Some(next) = map.get_mut(*p) else {
                        return;
                    };
                    cur = next;
                }
                _ => return,
            }
        }

        if let Value::Object(map) = cur {
            map.remove(last);
        }
    }
}

#[async_trait]
impl<P> DogAfterHook<Value, P> for ProtectHook<P>
where
    P: ProtectHookParams + Clone + Send + Sync + 'static,
{
    async fn run(&self, ctx: &mut HookContext<Value, P>) -> Result<()> {
        let provider = ctx.params.provider().unwrap_or("");
        if provider.trim().is_empty() {
            return Ok(());
        }

        let Some(res) = ctx.result.take() else {
            return Ok(());
        };

        ctx.result = Some(match res {
            HookResult::One(v) => {
                // Also support the Feathers paginated shape { data: [...] }
                if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
                    let stripped: Vec<Value> = data.clone().into_iter().map(|x| self.strip_one(x)).collect();
                    let mut out = v;
                    if let Some(map) = out.as_object_mut() {
                        map.insert("data".to_string(), Value::Array(stripped));
                    }
                    HookResult::One(out)
                } else {
                    HookResult::One(self.strip_one(v))
                }
            }
            HookResult::Many(vs) => HookResult::Many(vs.into_iter().map(|v| self.strip_one(v)).collect()),
        });

        Ok(())
    }
}