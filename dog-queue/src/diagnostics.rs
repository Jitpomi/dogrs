//! Opt-in aggregate timings for diagnostic fixtures. No tenant IDs or payloads are recorded.
//! Enable `queue-diagnostics` and DOGRS_QUEUE_TIMINGS=1 before process startup.
//! Timings are inclusive wall time across concurrent tasks; nested totals must not be added.
// Backend-specific probes are unused when their optional backend is disabled.
#![allow(dead_code)]
use std::{
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::Instant,
};
pub(crate) const PG_ENQUEUE_TOTAL: usize = 0;
pub(crate) const PG_CLAIM_TOTAL: usize = 1;
pub(crate) const PG_COMPLETE_TOTAL: usize = 2;
pub(crate) const PG_ENQUEUE_SLOT: usize = 3;
pub(crate) const PG_POOL: usize = 4;
pub(crate) const PG_INSERT: usize = 5;
pub(crate) const PG_CLAIM_DISPATCH: usize = 6;
pub(crate) const PG_CLAIM_POOL: usize = 7;
pub(crate) const PG_CLAIM_SQL: usize = 8;
pub(crate) const PG_CLAIM_DECODE: usize = 9;
pub(crate) const PG_COMPLETE_DISPATCH: usize = 10;
pub(crate) const PG_COMPLETE_POOL: usize = 11;
pub(crate) const PG_COMPLETE_SQL: usize = 12;
pub(crate) const NATS_ENQUEUE_TOTAL: usize = 13;
pub(crate) const NATS_CLAIM_TOTAL: usize = 14;
pub(crate) const NATS_COMPLETE_TOTAL: usize = 15;
pub(crate) const NATS_ENQUEUE_SLOT: usize = 16;
pub(crate) const NATS_LEGACY: usize = 17;
pub(crate) const NATS_INDEX: usize = 18;
pub(crate) const NATS_PAYLOAD_CREATE: usize = 19;
pub(crate) const NATS_CAS: usize = 20;
pub(crate) const NATS_PAYLOAD_READ: usize = 21;
pub(crate) const NATS_DISCOVERY_WAIT: usize = 22;
pub(crate) const NATS_CANDIDATE: usize = 23;
pub(crate) const NATS_POINT_READ: usize = 24;
pub(crate) const PG_CLAIM_QUEUE: usize = 25;
pub(crate) const PG_COMPLETE_QUEUE: usize = 26;
pub(crate) const NATS_ATOMIC_COMMIT: usize = 27;
pub(crate) const NATS_ENQUEUE_COMMIT: usize = 28;
pub(crate) const NATS_ENQUEUE_BATCH_QUEUE: usize = 29;
pub(crate) const NATS_UPDATE_BATCH_QUEUE: usize = 30;
pub(crate) const NATS_ENQUEUE_BATCH_EXECUTE: usize = 31;
pub(crate) const NATS_UPDATE_BATCH_EXECUTE: usize = 32;
pub(crate) const NATS_ATOMIC_FINAL_WAIT: usize = 33;
pub(crate) const NATS_ATOMIC_SEND: usize = 34;
pub(crate) const NATS_BATCH_REJECTED: usize = 35;
pub(crate) const NATS_ATOMIC_FIRST_STAGING_WAIT: usize = 36;
pub(crate) const NATS_ATOMIC_AFTER_STAGING_WAIT: usize = 37;
const NAMES: &[&str] = &[
    "pg_enqueue_total",
    "pg_claim_total",
    "pg_complete_total",
    "pg_enqueue_slot",
    "pg_pool",
    "pg_insert",
    "pg_claim_dispatch",
    "pg_claim_pool",
    "pg_claim_sql",
    "pg_claim_decode",
    "pg_complete_dispatch",
    "pg_complete_pool",
    "pg_complete_sql",
    "nats_enqueue_total",
    "nats_claim_total",
    "nats_complete_total",
    "nats_enqueue_slot",
    "nats_legacy",
    "nats_index",
    "nats_payload_create",
    "nats_cas",
    "nats_payload_read",
    "nats_discovery_wait",
    "nats_candidate",
    "nats_point_read",
    "pg_claim_queue",
    "pg_complete_queue",
    "nats_atomic_commit",
    "nats_enqueue_commit",
    "nats_enqueue_batch_queue",
    "nats_update_batch_queue",
    "nats_enqueue_batch_execute",
    "nats_update_batch_execute",
    "nats_atomic_final_wait",
    "nats_atomic_send",
    "nats_batch_rejected",
    "nats_atomic_first_staging_wait",
    "nats_atomic_after_staging_wait",
];
// Logical publish work, not disk writes or fsync calls. Rejected attempts are
// counted separately from validated durable acknowledgements.
const WRITE_NAMES: [&str; 8] = [
    "attempts",
    "attempted_messages",
    "attempted_value_bytes",
    "acknowledged_commits",
    "acknowledged_messages",
    "acknowledged_value_bytes",
    "conflicts",
    "errors",
];
static WRITES: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
pub(crate) fn publish_attempt(messages: usize, bytes: usize) {
    if enabled() {
        for (index, amount) in [(0, 1), (1, messages), (2, bytes)] {
            WRITES[index].fetch_add(amount as u64, Ordering::Relaxed);
        }
    }
}
pub(crate) fn publish_result(messages: usize, bytes: usize, committed: Option<bool>) {
    if enabled() {
        match committed {
            Some(true) => {
                for (index, amount) in [(3, 1), (4, messages), (5, bytes)] {
                    WRITES[index].fetch_add(amount as u64, Ordering::Relaxed);
                }
            }
            Some(false) => {
                WRITES[6].fetch_add(1, Ordering::Relaxed);
            }
            None => {
                WRITES[7].fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
struct Metric {
    count: AtomicU64,
    canceled: AtomicU64,
    nanos: AtomicU64,
    max: AtomicU64,
    buckets: [AtomicU64; 32],
}
impl Metric {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            canceled: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
            max: AtomicU64::new(0),
            buckets: [const { AtomicU64::new(0) }; 32],
        }
    }
}
static METRICS: [Metric; NAMES.len()] = [const { Metric::new() }; NAMES.len()];
fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    cfg!(feature = "queue-diagnostics")
        && *ON.get_or_init(|| std::env::var("DOGRS_QUEUE_TIMINGS").as_deref() == Ok("1"))
}
pub(crate) fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}
fn record(stage: usize, start: Option<Instant>, canceled: bool) {
    if let Some(start) = start {
        let ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let m = &METRICS[stage];
        m.count.fetch_add(1, Ordering::Relaxed);
        m.canceled.fetch_add(u64::from(canceled), Ordering::Relaxed);
        m.nanos.fetch_add(ns, Ordering::Relaxed);
        m.max.fetch_max(ns, Ordering::Relaxed);
        let micros = ns.div_ceil(1000).max(1);
        let bucket = (64 - (micros - 1).leading_zeros()).min(31) as usize;
        m.buckets[bucket].fetch_add(1, Ordering::Relaxed);
    }
}
pub(crate) fn elapsed(stage: usize, start: Option<Instant>) {
    record(stage, start, false);
}
pub(crate) struct Scope {
    stage: usize,
    start: Option<Instant>,
}
impl Scope {
    pub(crate) fn new(stage: usize) -> Self {
        Self {
            stage,
            start: start(),
        }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        elapsed(self.stage, self.start);
    }
}
struct AwaitTimer {
    stage: usize,
    start: Option<Instant>,
    done: bool,
}
impl Drop for AwaitTimer {
    fn drop(&mut self) {
        record(self.stage, self.start, !self.done);
    }
}
pub(crate) async fn measure<F: Future>(stage: usize, future: F) -> F::Output {
    let mut timer = AwaitTimer {
        stage,
        start: start(),
        done: false,
    };
    let result = future.await;
    timer.done = true;
    result
}
/// Cumulative diagnostic snapshot. Take after workload tasks stop, before verification reads.
/// `p95_upper_us` is a power-of-two histogram bound, not an exact percentile.
#[doc(hidden)]
pub fn snapshot() -> serde_json::Value {
    if !enabled() {
        return serde_json::Value::Null;
    }
    let mut out = serde_json::Map::new();
    out.insert(
        "nats_publish_work".into(),
        serde_json::Value::Object(
            WRITE_NAMES
                .iter()
                .zip(&WRITES)
                .map(|(name, value)| {
                    (
                        (*name).into(),
                        serde_json::json!(value.load(Ordering::Relaxed)),
                    )
                })
                .collect(),
        ),
    );
    for (name, m) in NAMES.iter().zip(&METRICS) {
        let count = m.count.load(Ordering::Relaxed);
        if count == 0 {
            continue;
        }
        let ns = m.nanos.load(Ordering::Relaxed);
        let mut cumulative = 0;
        let mut p95 = 0;
        for (i, b) in m.buckets.iter().enumerate() {
            cumulative += b.load(Ordering::Relaxed);
            if cumulative >= (count * 95).div_ceil(100) {
                p95 = 1u64 << i;
                break;
            }
        }
        out.insert((*name).into(), serde_json::json!({"count":count,"canceled":m.canceled.load(Ordering::Relaxed),"total_ms":ns as f64/1e6,"mean_ms":ns as f64/1e6/count as f64,"max_ms":m.max.load(Ordering::Relaxed) as f64/1e6,"p95_upper_us":p95}));
    }
    serde_json::Value::Object(out)
}

#[cfg(all(test, feature = "queue-diagnostics"))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn records_waits_and_cancellation_without_changing_results() {
        // This unit test does not modify process environment shared by other tests.
        let start = Some(Instant::now());
        record(PG_ENQUEUE_TOTAL, start, false);
        let before = METRICS[PG_ENQUEUE_TOTAL].count.load(Ordering::Relaxed);
        let result: Result<u8, &str> = measure(PG_ENQUEUE_TOTAL, async { Err("original") }).await;
        assert_eq!(result, Err("original"));
        assert!(METRICS[PG_ENQUEUE_TOTAL].count.load(Ordering::Relaxed) >= before);
        let timer = AwaitTimer {
            stage: NATS_ENQUEUE_TOTAL,
            start: Some(Instant::now()),
            done: false,
        };
        drop(timer);
        assert_eq!(
            METRICS[NATS_ENQUEUE_TOTAL].canceled.load(Ordering::Relaxed),
            1
        );
        assert_eq!(METRICS[NATS_ENQUEUE_TOTAL].count.load(Ordering::Relaxed), 1);
    }
}
