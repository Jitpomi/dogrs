//! Shared delayed delivery and retry timing. Application code supplies policy,
//! never runs its own timer or repeats a provider operation here.
use crate::{QueueError, QueueResult};
use chrono::{DateTime, Utc};
pub fn delayed_at(now: DateTime<Utc>, seconds: u64) -> QueueResult<DateTime<Utc>> {
    let seconds =
        i64::try_from(seconds).map_err(|_| QueueError::InvalidConfig("Delay exhausted".into()))?;
    let delta = chrono::Duration::try_seconds(seconds)
        .ok_or_else(|| QueueError::InvalidConfig("Delay exhausted".into()))?;
    now.checked_add_signed(delta)
        .ok_or_else(|| QueueError::InvalidConfig("UTC time exhausted".into()))
}
pub fn retry_at(
    now: DateTime<Utc>,
    retry_index: u32,
    base_seconds: u64,
    cap_seconds: u64,
    full_jitter: bool,
) -> QueueResult<DateTime<Utc>> {
    if base_seconds > cap_seconds {
        return Err(QueueError::InvalidConfig("Retry base exceeds cap".into()));
    }
    let multiplier = 1u64.checked_shl(retry_index).unwrap_or(u64::MAX);
    let ceiling = base_seconds.saturating_mul(multiplier).min(cap_seconds);
    let seconds = if full_jitter && ceiling > 0 {
        rand::random_range(0..=ceiling)
    } else {
        ceiling
    };
    delayed_at(now, seconds)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_are_bounded_even_after_billions_of_attempts() {
        let now = Utc::now();
        assert_eq!(
            retry_at(now, u32::MAX, 1, 64, false).unwrap().timestamp() - now.timestamp(),
            64
        );
        assert!(delayed_at(now, u64::MAX).is_err());
        assert!(retry_at(now, 0, 2, 1, false).is_err());
    }
}
