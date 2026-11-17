//! The words of the retry row, drawn in place of "Working" while a model call is tried
//! again.

use std::time::Instant;

use dusk_protocol::thread::Retry;

/// The row's line: the core's text and the time left before the next attempt, which the
/// ticking of the working figure redraws.
pub fn line(retry: &Retry, arrived: Instant) -> String {
    let elapsed = u64::try_from(arrived.elapsed().as_millis()).unwrap_or(u64::MAX);
    format!(
        "{} \u{00B7} {}",
        retry.text(),
        countdown(retry.wait_ms, elapsed)
    )
}

fn countdown(wait_ms: u64, elapsed_ms: u64) -> String {
    match wait_ms.saturating_sub(elapsed_ms) {
        0 => "trying now".to_owned(),
        left => format!("next in {} s", left.div_ceil(1000)),
    }
}

#[cfg(test)]
mod tests {
    use super::countdown;

    #[test]
    fn the_countdown_rounds_up_and_ends_in_trying_now() {
        assert_eq!(countdown(8000, 0), "next in 8 s");
        assert_eq!(countdown(8000, 7500), "next in 1 s");
        assert_eq!(countdown(8000, 8000), "trying now");
        assert_eq!(countdown(1000, 9000), "trying now");
    }
}
