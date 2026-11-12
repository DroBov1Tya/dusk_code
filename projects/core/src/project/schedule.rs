//! When a scheduled task runs: a draft from a client is checked here, and the next run
//! is worked out on this machine's clock.

use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Datelike, Days, NaiveTime, TimeZone};
use dusk_protocol::project::{
    Clock, MAX_INTERVAL_MINUTES, MAX_PROMPT_CHARS, MIN_INTERVAL_MINUTES, Repeat, ScheduleDraft,
};

/// Refuses a draft that could not run or would run too often.
pub fn check(draft: &ScheduleDraft, now_ms: i64) -> Result<()> {
    let prompt = draft.prompt.trim();
    ensure!(!prompt.is_empty(), "write what the task should do");
    ensure!(
        prompt.chars().count() <= MAX_PROMPT_CHARS,
        "the prompt is longer than {MAX_PROMPT_CHARS} characters"
    );
    match draft.repeat {
        Repeat::Once { at } => ensure!(at > now_ms, "the time has already passed"),
        Repeat::Interval { minutes } => ensure!(
            (MIN_INTERVAL_MINUTES..=MAX_INTERVAL_MINUTES).contains(&minutes),
            "the interval is {MIN_INTERVAL_MINUTES} minutes to a week"
        ),
        Repeat::Weekly { day, .. } if day > 6 => bail!("the day of the week is 0 to 6"),
        Repeat::Custom { days, .. } if days == 0 || days > 0b111_1111 => {
            bail!("choose at least one day")
        }
        _ => {}
    }
    if let Some((_, at)) = draft.repeat.days_and_time() {
        ensure!(
            at.hour < 24 && at.minute < 60,
            "the time is not a time of day"
        );
    }
    Ok(())
}

/// The first run after `now`, or `None` for a single run whose time has passed.
pub fn next_run<Tz: TimeZone>(repeat: &Repeat, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
    match *repeat {
        Repeat::Once { at } => {
            let at = now.timezone().timestamp_millis_opt(at).single()?;
            (at > *now).then_some(at)
        }
        Repeat::Interval { minutes } => {
            Some(now.clone() + chrono::Duration::minutes(i64::from(minutes)))
        }
        _ => {
            let (days, at) = repeat.days_and_time()?;
            next_on_days(days, at, now)
        }
    }
}

/// The first chosen day, today included, whose time is still ahead.
fn next_on_days<Tz: TimeZone>(days: u8, at: Clock, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
    let time = NaiveTime::from_hms_opt(u32::from(at.hour), u32::from(at.minute), 0)?;
    let today = now.date_naive();
    // Eight days: today's time may have passed, and the same weekday comes again.
    (0..=7).find_map(|offset| {
        let date = today.checked_add_days(Days::new(offset))?;
        let bit = 1u8 << date.weekday().num_days_from_monday();
        if days & bit == 0 {
            return None;
        }
        // A time skipped by a clock change runs at the earliest moment that exists.
        let local = now.timezone().from_local_datetime(&date.and_time(time));
        let moment = local.earliest().or_else(|| {
            let later = date.and_time(time) + chrono::Duration::hours(1);
            now.timezone().from_local_datetime(&later).earliest()
        })?;
        (moment > *now).then_some(moment)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dusk_protocol::project::WEEKENDS;

    // 2026-11-13 is a Friday.
    fn friday(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 13, hour, minute, 0).unwrap()
    }

    fn clock(hour: u8, minute: u8) -> Clock {
        Clock { hour, minute }
    }

    fn draft(repeat: Repeat) -> ScheduleDraft {
        ScheduleDraft {
            id: None,
            project_id: "p".into(),
            prompt: "check the build".into(),
            repeat,
            enabled: true,
        }
    }

    #[test]
    fn a_daily_run_is_today_until_its_time_then_tomorrow() {
        let daily = Repeat::Daily { at: clock(9, 30) };
        assert_eq!(next_run(&daily, &friday(8, 0)), Some(friday(9, 30)));
        assert_eq!(
            next_run(&daily, &friday(9, 30)),
            Some(Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap())
        );
    }

    #[test]
    fn weekends_skip_to_saturday_and_weekly_comes_back_in_a_week() {
        let weekends = Repeat::Custom {
            days: WEEKENDS,
            at: clock(10, 0),
        };
        assert_eq!(
            next_run(&weekends, &friday(12, 0)),
            Some(Utc.with_ymd_and_hms(2026, 11, 14, 10, 0, 0).unwrap())
        );
        let weekly = Repeat::Weekly {
            day: 4,
            at: clock(7, 0),
        };
        assert_eq!(
            next_run(&weekly, &friday(7, 0)),
            Some(Utc.with_ymd_and_hms(2026, 11, 20, 7, 0, 0).unwrap())
        );
    }

    #[test]
    fn a_single_run_in_the_past_never_comes() {
        let at = friday(6, 0).timestamp_millis();
        assert_eq!(next_run(&Repeat::Once { at }, &friday(8, 0)), None);
        assert!(check(&draft(Repeat::Once { at }), friday(8, 0).timestamp_millis()).is_err());
    }

    #[test]
    fn drafts_that_cannot_run_or_run_too_often_are_refused() {
        let now = friday(8, 0).timestamp_millis();
        let refused = [
            draft(Repeat::Interval { minutes: 1 }),
            draft(Repeat::Interval {
                minutes: MAX_INTERVAL_MINUTES + 1,
            }),
            draft(Repeat::Custom {
                days: 0,
                at: clock(9, 0),
            }),
            draft(Repeat::Weekly {
                day: 7,
                at: clock(9, 0),
            }),
            draft(Repeat::Daily { at: clock(24, 0) }),
            draft(Repeat::Daily { at: clock(9, 60) }),
            ScheduleDraft {
                prompt: "   ".into(),
                ..draft(Repeat::Daily { at: clock(9, 0) })
            },
            ScheduleDraft {
                prompt: "x".repeat(MAX_PROMPT_CHARS + 1),
                ..draft(Repeat::Daily { at: clock(9, 0) })
            },
        ];
        for draft in refused {
            assert!(check(&draft, now).is_err(), "{:?}", draft.repeat);
        }
        assert!(check(&draft(Repeat::Interval { minutes: 60 }), now).is_ok());
    }
}
