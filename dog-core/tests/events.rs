use dog_core::{
    events::{EventListener, ServiceEventPattern},
    DogApp, DogAppBuilder, DogEventHub, DogService, HookContext, ServiceCaller, ServiceEventData,
    ServiceEventKind, ServiceMethodKind, TenantContext,
};
use futures::{executor::block_on, future::pending, task::noop_waker};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

type App = DogApp<String, ()>;
fn context(app: App) -> HookContext<String, ()> {
    HookContext::new(
        TenantContext::new("t"),
        ServiceMethodKind::Create,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    )
}
fn count(counter: Arc<AtomicUsize>) -> EventListener<String, ()> {
    Arc::new(move |_, _| {
        let counter = counter.clone();
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    })
}
struct Echo;
#[async_trait::async_trait]
impl DogService<String, ()> for Echo {
    async fn create(&self, _: &TenantContext, data: String, _: ()) -> anyhow::Result<String> {
        Ok(data)
    }
}

#[test]
fn standard_and_custom_errors_are_counted_without_losing_success_or_later_listeners() {
    block_on(async {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = DogAppBuilder::new();
        builder.register_service("echo", Arc::new(Echo));
        for kind in [
            ServiceEventKind::Created,
            ServiceEventKind::custom("notice"),
        ] {
            builder.on(
                "echo",
                kind.clone(),
                Arc::new(|_, _| Box::pin(async { anyhow::bail!("private detail") })),
            );
            builder.on("echo", kind, count(calls.clone()));
        }
        let app = builder.build();
        assert_eq!(
            app.service("echo")
                .unwrap()
                .create(TenantContext::new("t"), "saved".into(), ())
                .await
                .unwrap(),
            "saved"
        );
        app.emit_custom("echo", "notice", Arc::new(()), &context(app.clone()))
            .await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(app.event_listener_failures(), 2);
        assert_eq!(app.clone().event_listener_failures(), 2);
    });
}

#[test]
fn direct_hub_error_is_counted_and_preserves_short_circuit_contract() {
    block_on(async {
        let mut hub = DogEventHub::new();
        let calls = Arc::new(AtomicUsize::new(0));
        hub.on_exact(
            "echo",
            ServiceEventKind::Created,
            Arc::new(|_, _| Box::pin(async { anyhow::bail!("failed") })),
        );
        hub.on_exact("echo", ServiceEventKind::Created, count(calls.clone()));
        let payload: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
        let ctx = context(DogAppBuilder::new().build());
        assert!(hub
            .emit_async(
                "echo",
                &ServiceEventKind::Created,
                &ServiceEventData::Custom(&payload),
                &ctx
            )
            .await
            .is_err());
        assert_eq!(hub.listener_failures(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn once_selection_is_atomic_across_concurrent_emitters() {
    let mut hub = DogEventHub::new();
    let calls = Arc::new(AtomicUsize::new(0));
    hub.once_pattern(
        ServiceEventPattern::exact("echo", ServiceEventKind::Created),
        count(calls.clone()),
    );
    let hub = Arc::new(hub);
    let barrier = Arc::new(std::sync::Barrier::new(16));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let hub = hub.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let payload: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
                let ctx = context(DogAppBuilder::new().build());
                barrier.wait();
                block_on(hub.emit_async(
                    "echo",
                    &ServiceEventKind::Created,
                    &ServiceEventData::Custom(&payload),
                    &ctx,
                ))
                .unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn publish_rejection_does_not_consume_once_listener() {
    let mut hub = DogEventHub::new();
    hub.once_pattern(
        ServiceEventPattern::exact("echo", ServiceEventKind::Created),
        count(Arc::new(AtomicUsize::new(0))),
    );
    let payload: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let data = ServiceEventData::Custom(&payload);
    let ctx = context(DogAppBuilder::new().build());
    hub.set_publish(Arc::new(|_, _, _, _| false));
    assert!(hub
        .snapshot_emit("echo", &ServiceEventKind::Created, &data, &ctx)
        .is_empty());
    hub.clear_publish();
    assert_eq!(
        hub.snapshot_emit("echo", &ServiceEventKind::Created, &data, &ctx)
            .len(),
        1
    );
    assert!(hub
        .snapshot_emit("echo", &ServiceEventKind::Created, &data, &ctx)
        .is_empty());
}

#[test]
fn cancelled_snapshot_does_not_rearm_once_listener() {
    let mut hub = DogEventHub::new();
    hub.once_pattern(
        ServiceEventPattern::exact("echo", ServiceEventKind::Created),
        count(Arc::new(AtomicUsize::new(0))),
    );
    let payload: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let data = ServiceEventData::Custom(&payload);
    let ctx = context(DogAppBuilder::new().build());
    drop(hub.snapshot_emit("echo", &ServiceEventKind::Created, &data, &ctx));
    assert!(hub
        .snapshot_emit("echo", &ServiceEventKind::Created, &data, &ctx)
        .is_empty());
}

#[test]
fn pending_listener_blocks_later_delivery_and_is_dropped_on_cancellation() {
    struct Guard(Arc<AtomicUsize>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let active = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = DogAppBuilder::new();
    builder.on(
        "echo",
        ServiceEventKind::custom("notice"),
        Arc::new({
            let active = active.clone();
            move |_, _| {
                let active = active.clone();
                Box::pin(async move {
                    active.fetch_add(1, Ordering::SeqCst);
                    let _guard = Guard(active);
                    pending::<()>().await;
                    Ok(())
                })
            }
        }),
    );
    builder.on(
        "echo",
        ServiceEventKind::custom("notice"),
        count(calls.clone()),
    );
    let app = builder.build();
    let ctx = context(app.clone());
    let mut emit = Box::pin(app.emit_custom("echo", "notice", Arc::new(()), &ctx));
    assert!(matches!(
        emit.as_mut().poll(&mut Context::from_waker(&noop_waker())),
        Poll::Pending
    ));
    assert_eq!(active.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(emit);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(app.event_listener_failures(), 0);
}
