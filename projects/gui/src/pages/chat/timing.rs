//! How long the agent worked on a turn: from the boundary that opened it to the one that
//! closed it, both stamped on the clock, so a chat read from history shows it too.

/// When a turn began and, once it is over, when it ended, in Unix milliseconds.
pub type Span = (i64, Option<i64>);

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// "8s", "2m 14s", "1h 3m".
pub fn duration(ms: i64) -> String {
    let seconds = (ms.max(0) + 500) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_in_the_largest_two_units() {
        assert_eq!(duration(8_400), "8s");
        assert_eq!(duration(134_000), "2m 14s");
        assert_eq!(duration(3_780_000), "1h 3m");
        assert_eq!(duration(-5), "0s");
    }
}
