use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};

pub fn parse_duration(input: &str) -> Result<TimeDelta> {
    let std_duration =
        humantime::parse_duration(input).with_context(|| format!("invalid duration: {input}"))?;
    let millis = std_duration
        .as_millis()
        .try_into()
        .with_context(|| "duration too large")?;
    TimeDelta::try_milliseconds(millis).with_context(|| "duration out of range")
}

pub fn compute_destroy_at(duration_str: &str) -> Result<DateTime<Utc>> {
    let delta = parse_duration(duration_str)?;
    Ok(Utc::now() + delta)
}

const BACKOFF_SECONDS: [i64; 3] = [0, 300, 900];

pub fn backoff_for_attempt(attempt: u32) -> Option<TimeDelta> {
    let idx = attempt as usize;
    if idx >= BACKOFF_SECONDS.len() {
        return None;
    }
    TimeDelta::try_seconds(BACKOFF_SECONDS[idx])
}

pub fn is_retry_eligible(attempt: u32, last_attempt_at: Option<DateTime<Utc>>) -> bool {
    let Some(backoff) = backoff_for_attempt(attempt) else {
        return false;
    };
    match (attempt, last_attempt_at) {
        (0, _) => true,
        (_, None) => true,
        (_, Some(last)) => Utc::now() >= last + backoff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_seconds() {
        let delta = parse_duration("30s").unwrap();
        assert_eq!(delta.num_seconds(), 30);
    }

    #[test]
    fn test_parse_duration_hours() {
        let delta = parse_duration("1h").unwrap();
        assert_eq!(delta.num_seconds(), 3600);
    }

    #[test]
    fn test_parse_duration_days() {
        let delta = parse_duration("1d").unwrap();
        assert_eq!(delta.num_seconds(), 86400);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn test_compute_destroy_at_is_future() {
        let result = compute_destroy_at("1h").unwrap();
        assert!(result > Utc::now());
    }

    #[test]
    fn test_backoff_attempt_0() {
        let b = backoff_for_attempt(0).unwrap();
        assert_eq!(b.num_seconds(), 0);
    }

    #[test]
    fn test_backoff_attempt_1() {
        let b = backoff_for_attempt(1).unwrap();
        assert_eq!(b.num_seconds(), 300);
    }

    #[test]
    fn test_backoff_attempt_2() {
        let b = backoff_for_attempt(2).unwrap();
        assert_eq!(b.num_seconds(), 900);
    }

    #[test]
    fn test_backoff_attempt_3_is_none() {
        assert!(backoff_for_attempt(3).is_none());
    }

    #[test]
    fn test_retry_eligible_attempt_0() {
        assert!(is_retry_eligible(0, None));
    }

    #[test]
    fn test_retry_eligible_attempt_3_not_eligible() {
        assert!(!is_retry_eligible(3, None));
    }
}
