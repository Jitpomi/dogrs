//! Preserve human duration settings using a maintained upstream parser.
use fundu::{CustomDurationParser, CustomTimeUnit, Multiplier, TimeUnit::*};
use serde::{Deserialize, Deserializer, Serializer};
use std::time::Duration;

pub fn serialize<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    // Exact canonical seconds retain every nanosecond and remain readable by
    // existing consumers. The configuration wire type stays a string.
    let text = if value.subsec_nanos() == 0 {
        format!("{}s", value.as_secs())
    } else {
        format!("{}.{:09}s", value.as_secs(), value.subsec_nanos())
    };
    serializer.serialize_str(&text)
}
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}
fn parse(text: &str) -> Result<Duration, &'static str> {
    // Every term must carry a unit. Fundu also supports unitless seconds and
    // signed/infinite values, which are inappropriate for token lifetimes.
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = normalized.as_str();
    let mut at = 0;
    let bytes = text.as_bytes();
    let mut terms = 0;
    while at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        if at == start {
            return Err("A token duration needs a nonnegative number and unit");
        }
        if bytes.get(at) == Some(&b'.') {
            at += 1;
            let start = at;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            if at == start {
                return Err("Invalid duration fraction");
            }
        }
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && (bytes[at].is_ascii_alphabetic() || bytes[at] >= 128) {
            at += 1;
        }
        if at == start {
            return Err("A token duration needs an explicit unit");
        }
        terms += 1;
    }
    if terms == 0 {
        return Err("Duration is empty");
    }
    let units = [
        CustomTimeUnit::with_default(NanoSecond, &["nanos", "nsec", "ns"]),
        CustomTimeUnit::with_default(MicroSecond, &["usec", "us", "µs"]),
        CustomTimeUnit::with_default(MilliSecond, &["millis", "msec", "ms"]),
        CustomTimeUnit::with_default(Second, &["seconds", "second", "secs", "sec", "s"]),
        CustomTimeUnit::with_default(Minute, &["minutes", "minute", "min", "mins", "m"]),
        CustomTimeUnit::with_default(Hour, &["hours", "hour", "hr", "hrs", "h"]),
        CustomTimeUnit::with_default(Day, &["days", "day", "d"]),
        CustomTimeUnit::with_default(Week, &["weeks", "week", "wk", "wks", "w"]),
        // Preserve the existing fixed configuration meanings, not calendar math.
        CustomTimeUnit::new(
            Second,
            &["months", "month", "M"],
            Some(Multiplier(2_630_016, 0)),
        ),
        CustomTimeUnit::new(
            Second,
            &["years", "year", "yr", "yrs", "y"],
            Some(Multiplier(31_557_600, 0)),
        ),
    ];
    let mut parser = CustomDurationParser::with_time_units(&units);
    parser
        .allow_time_unit_delimiter(true)
        .parse_multiple(true, None)
        .disable_exponent(true)
        .disable_infinity(true);
    let duration: Duration = parser
        .parse(text)
        .map_err(|_| "Invalid token duration")?
        .try_into()
        .map_err(|_| "Token duration is out of range")?;
    // Fundu saturates enormous values. Token arithmetic uses checked i64 Unix
    // seconds; reject unrepresentable settings rather than silently saturating.
    if duration.as_secs() > i64::MAX as u64 {
        return Err("Token duration is out of range");
    }
    Ok(duration)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Config {
        #[serde(with = "super")]
        value: Duration,
    }
    #[test]
    fn existing_units_and_exact_roundtrips() {
        for (text, value) in [
            ("1h 30m", Duration::from_secs(5400)),
            ("1h\u{2003}30m", Duration::from_secs(5400)),
            ("1month", Duration::from_secs(2_630_016)),
            ("1year", Duration::from_secs(31_557_600)),
            ("3usec", Duration::from_nanos(3000)),
            ("17nanos", Duration::from_nanos(17)),
            ("1.25s", Duration::new(1, 250_000_000)),
            ("0s", Duration::ZERO),
            ("1h30mins", Duration::from_secs(5400)),
        ] {
            assert_eq!(parse(text).unwrap(), value, "{text}");
            let json = serde_json::to_string(&Config { value }).unwrap();
            assert_eq!(serde_json::from_str::<Config>(&json).unwrap().value, value);
        }
    }
    #[test]
    fn unsafe_or_ambiguous_lifetimes_fail_closed() {
        for text in [
            "",
            "1",
            "-1s",
            "+1s",
            "inf",
            "1e9s",
            "1h 2",
            "1. s",
            "999999999999999999999999999years",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }
}
