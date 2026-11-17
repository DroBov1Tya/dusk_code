//! The words of the retry row, which take the place of "Working" while a model call is
//! tried again.

use super::ChatData;

impl ChatData {
    /// The core's text and the time left before the next attempt, at `now` in Unix
    /// milliseconds; none when no call is tried again.
    pub fn retry_line(&self, now: i64) -> Option<String> {
        let (retry, arrived) = self.retry.as_ref()?;
        let left = i64::try_from(retry.wait_ms)
            .unwrap_or(i64::MAX)
            .saturating_sub(now.saturating_sub(*arrived));
        let countdown = match left {
            ..=0 => "trying now".to_owned(),
            left => format!("next in {} s", (left + 999) / 1000),
        };
        Some(format!("{} \u{00B7} {countdown}", retry.text()))
    }
}
