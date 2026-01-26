// Protect hook.

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::DogAfterHook;
use dog_core::{HookContext, HookResult};
use serde_json::Value;
use std::collections::HashSet;

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
    /// Path-based removal rules (supports dotted paths like "authentication.accessToken")
    paths: Vec<String>,
    /// Deep removal rules (remove matching keys anywhere in the JSON tree)
    deep_fields: HashSet<String>,
    _phantom: std::marker::PhantomData<P>,
}

impl<P> ProtectHook<P>
where
    P: ProtectHookParams + 'static,
{
    pub fn new(fields: Vec<String>) -> Self {
        Self {
            paths: fields,
            deep_fields: HashSet::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn from_fields(fields: &[&str]) -> Self {
        Self::new(fields.iter().map(|s| s.to_string()).collect())
    }

    /// Remove the given keys anywhere in the JSON tree (deep recursive stripping).
    ///
    /// This is intentionally separate from `from_fields`, which preserves the original
    /// dotted-path semantics for backward compatibility.
    pub fn from_deep_fields(fields: &[&str]) -> Self {
        let mut out = Self::new(vec![]);
        out.deep_fields = fields.iter().map(|s| s.to_string()).collect();
        out
    }

    /// Add dotted-path removal rules (e.g. "authentication.accessToken").
    pub fn with_paths(mut self, paths: &[&str]) -> Self {
        self.paths
            .extend(paths.iter().map(|s| s.to_string()));
        self
    }

    /// Add deep recursive removal rules (e.g. remove "password" anywhere).
    pub fn with_deep_fields(mut self, fields: &[&str]) -> Self {
        for f in fields {
            self.deep_fields.insert(f.to_string());
        }
        self
    }

    fn strip_one(&self, mut v: Value) -> Value {
        for p in &self.paths {
            Self::remove_path(&mut v, p);
        }

        if !self.deep_fields.is_empty() {
            Self::remove_deep_fields(&mut v, &self.deep_fields);
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

    fn remove_deep_fields(v: &mut Value, fields: &HashSet<String>) {
        match v {
            Value::Object(map) => {
                // Remove matching keys at this level
                for f in fields {
                    map.remove(f);
                }
                // Recurse into remaining values
                for (_, child) in map.iter_mut() {
                    Self::remove_deep_fields(child, fields);
                }
            }
            Value::Array(items) => {
                for child in items.iter_mut() {
                    Self::remove_deep_fields(child, fields);
                }
            }
            _ => {}
        }
    }
}

#[async_trait]
impl<P> DogAfterHook<Value, P> for ProtectHook<P>
where
    P: ProtectHookParams + Clone + Send + Sync + 'static,
{
    async fn run(&self, ctx: &mut HookContext<Value, P>) -> Result<()> {
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