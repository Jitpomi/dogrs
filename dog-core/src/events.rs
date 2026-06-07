use std::any::Any;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use anyhow::Result;

use crate::hooks::HookFut;
use crate::{HookContext, HookResult, ServiceMethodKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

static LISTENER_ID: AtomicU64 = AtomicU64::new(1);

fn next_listener_id() -> ListenerId {
    ListenerId(LISTENER_ID.fetch_add(1, Ordering::Relaxed))
}

/// Feathers standard event names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ServiceEventKind {
    Created,
    Updated,
    Patched,
    Removed,
    Custom(String),
}

impl ServiceEventKind {
    pub fn custom(name: impl Into<String>) -> Self {
        ServiceEventKind::Custom(name.into())
    }
}

/// Data delivered to event listeners.
pub enum ServiceEventData<'a, R> {
    Standard(&'a HookResult<R>),
    Custom(&'a Arc<dyn Any + Send + Sync>),
}

/// Listener signature (async).
pub type EventListener<R, P> = Arc<
    dyn for<'a> Fn(&'a ServiceEventData<'a, R>, &'a HookContext<R, P>) -> HookFut<'a> + Send + Sync,
>;

/// publish gate: return true to deliver, false to skip.
pub type PublishFn<R, P> = Arc<
    dyn for<'a> Fn(
            &'a str,
            &'a ServiceEventKind,
            &'a ServiceEventData<'a, R>,
            &'a HookContext<R, P>,
        ) -> bool
        + Send
        + Sync,
>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ServiceNamePat {
    Any,
    Exact(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventPat {
    Any,
    Exact(ServiceEventKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServiceEventPattern {
    pub service: ServiceNamePat,
    pub event: EventPat,
}

impl ServiceEventPattern {
    pub fn exact(service: impl Into<String>, event: ServiceEventKind) -> Self {
        Self {
            service: ServiceNamePat::Exact(service.into()),
            event: EventPat::Exact(event),
        }
    }

    pub fn matches(&self, path: &str, event: &ServiceEventKind) -> bool {
        let service_ok = match &self.service {
            ServiceNamePat::Any => true,
            ServiceNamePat::Exact(s) => s == path,
        };
        let event_ok = match &self.event {
            EventPat::Any => true,
            EventPat::Exact(e) => e == event,
        };
        service_ok && event_ok
    }
}

#[derive(Clone)]
struct ListenerEntry<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    id: ListenerId,
    pattern: ServiceEventPattern,
    listener: EventListener<R, P>,
    once: bool,
    called: Arc<std::sync::atomic::AtomicBool>,
}

/// Minimal runtime-agnostic event hub.
///
/// IMPORTANT DESIGN:
/// - We do NOT want callers to need `&mut DogEventHub` just to emit, because
///   DogApp holds this behind an `RwLock`.
/// - We also do NOT want to hold a lock across `.await`.
///
/// So we split emission into:
/// 1) snapshot (read-only, no await)
/// 2) await listeners (no lock held)
/// 3) cleanup once-listeners (write-lock, no await)
pub struct DogEventHub<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    listeners: Vec<ListenerEntry<R, P>>,
    publish: Option<PublishFn<R, P>>,
}

impl<R, P> Default for DogEventHub<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P> DogEventHub<R, P>
where
    R: Send + 'static,
    P: Send + Clone + 'static,
{
    pub fn new() -> Self {
        Self {
            listeners: Vec::new(),
            publish: None,
        }
    }

    pub fn set_publish(&mut self, f: PublishFn<R, P>) {
        self.publish = Some(f);
    }

    pub fn clear_publish(&mut self) {
        self.publish = None;
    }

    /// Exact: app.on("messages", Created, ...)
    pub fn on_exact(
        &mut self,
        path: impl Into<String>,
        event: ServiceEventKind,
        listener: EventListener<R, P>,
    ) -> ListenerId {
        self.on_pattern(ServiceEventPattern::exact(path, event), listener)
    }

    /// Sugar: app.on_str("messages.created", ...)
    pub fn on_pattern(
        &mut self,
        pattern: ServiceEventPattern,
        listener: EventListener<R, P>,
    ) -> ListenerId {
        let id = next_listener_id();
        self.listeners.push(ListenerEntry {
            id,
            pattern,
            listener,
            once: false,
            called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        id
    }

    /// Feathers-ish: once(...)
    pub fn once_pattern(
        &mut self,
        pattern: ServiceEventPattern,
        listener: EventListener<R, P>,
    ) -> ListenerId {
        let id = next_listener_id();
        self.listeners.push(ListenerEntry {
            id,
            pattern,
            listener,
            once: true,
            called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        id
    }

    /// removeListener/off
    pub fn off(&mut self, id: ListenerId) -> bool {
        let before = self.listeners.len();
        self.listeners.retain(|e| e.id != id);
        before != self.listeners.len()
    }

    /// removeAllListeners (optionally scoped)
    pub fn remove_all(&mut self, pattern: Option<&ServiceEventPattern>) -> usize {
        let before = self.listeners.len();
        if let Some(p) = pattern {
            self.listeners.retain(|e| &e.pattern != p);
        } else {
            self.listeners.clear();
        }
        before - self.listeners.len()
    }

    ///
    /// NOTE: no `.await` here, so it’s safe under a read-lock (or entirely lock-free).
    pub fn snapshot_emit<'a>(
        &'a self,
        path: &str,
        event: &ServiceEventKind,
        data: &ServiceEventData<'a, R>,
        ctx: &HookContext<R, P>,
    ) -> Vec<EventListener<R, P>> {
        if let Some(publish) = &self.publish {
            if !(publish)(path, event, data, ctx) {
                return Vec::new();
            }
        }

        let mut to_call: Vec<EventListener<R, P>> = Vec::new();

        for entry in &self.listeners {
            if entry.pattern.matches(path, event) {
                if entry.once {
                    if entry.called.swap(true, Ordering::SeqCst) {
                        continue;
                    }
                }
                to_call.push(entry.listener.clone());
            }
        }

        to_call
    }

    /// Optional convenience if you ever hold `&self` directly.
    pub async fn emit_async(
        &self,
        path: &str,
        event: &ServiceEventKind,
        data: &ServiceEventData<'_, R>,
        ctx: &HookContext<R, P>,
    ) -> Result<()> {
        let listeners = self.snapshot_emit(path, event, data, ctx);

        for f in &listeners {
            f(data, ctx).await?;
        }

        Ok(())
    }
}

/// Feathers mapping: only these methods emit standard events.
pub fn method_to_standard_event(method: &ServiceMethodKind) -> Option<ServiceEventKind> {
    match method {
        ServiceMethodKind::Create => Some(ServiceEventKind::Created),
        ServiceMethodKind::Update => Some(ServiceEventKind::Updated),
        ServiceMethodKind::Patch => Some(ServiceEventKind::Patched),
        ServiceMethodKind::Remove => Some(ServiceEventKind::Removed),
        _ => None,
    }
}

/// Parse sugar strings like "messages.created", "messages.*", "*.*"
pub fn parse_event_pattern(input: &str) -> anyhow::Result<ServiceEventPattern> {
    let s = input.trim();

    let (svc, ev) = if let Some((a, b)) = s.split_once(' ') {
        (a.trim(), b.trim())
    } else if let Some((a, b)) = s.split_once('.') {
        (a.trim(), b.trim())
    } else {
        return Err(anyhow::anyhow!(
            "Invalid event pattern '{s}'. Expected 'service event' or 'service.event'."
        ));
    };

    let service = if svc == "*" {
        ServiceNamePat::Any
    } else {
        ServiceNamePat::Exact(svc.to_string())
    };

    let event = if ev == "*" {
        EventPat::Any
    } else {
        EventPat::Exact(parse_event_kind(ev)?)
    };

    Ok(ServiceEventPattern { service, event })
}

pub fn parse_event_kind(s: &str) -> anyhow::Result<ServiceEventKind> {
    let norm = s.trim().to_lowercase();
    match norm.as_str() {
        "created" => Ok(ServiceEventKind::Created),
        "updated" => Ok(ServiceEventKind::Updated),
        "patched" => Ok(ServiceEventKind::Patched),
        "removed" => Ok(ServiceEventKind::Removed),
        other => Ok(ServiceEventKind::Custom(other.to_string())),
    }
}

// Export ListenerId so DogApp can return it (kept for compatibility)
pub fn listener_id(id: ListenerId) -> ListenerId {
    id
}
