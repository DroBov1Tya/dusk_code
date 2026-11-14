//! `/schedule <when> <prompt>`: reads when a task runs from the words the user typed.
//! `daily 09:00`, `weekdays 09:00`, `weekends 10:30`, `weekly mon 09:00`, `every 2h`,
//! `every 30m`, `once 14:30` (today, or tomorrow once that time has passed).

use dusk_protocol::project::{Clock, DAY_NAMES, Repeat};

/// What a schedule action asks of the core.
pub enum ScheduleAction {
    /// `/schedules`: the list.
    Open,
    Add {
        repeat: Repeat,
        prompt: String,
    },
    Run(String),
    Delete(String),
    Toggle {
        id: String,
        on: bool,
    },
}

/// The repeat and the prompt from what follows `/schedule `; `now_ms` places a single run.
pub fn parse(text: &str, now: chrono::DateTime<chrono::Local>) -> Result<(Repeat, String), String> {
    const USAGE: &str = "write /schedule daily 09:00 <prompt>; also weekdays, weekends, \
                         weekly mon 09:00, every 2h, every 30m, once 14:30";
    let mut words = text.split_whitespace();
    let kind = words.next().ok_or(USAGE)?.to_lowercase();
    let clock = |word: Option<&str>| word.and_then(Clock::parse).ok_or(USAGE.to_owned());
    let repeat = match kind.as_str() {
        "daily" => Repeat::Daily {
            at: clock(words.next())?,
        },
        "weekdays" => Repeat::Weekdays {
            at: clock(words.next())?,
        },
        "weekends" => Repeat::Weekends {
            at: clock(words.next())?,
        },
        "weekly" => {
            let day = words.next().ok_or(USAGE)?.to_lowercase();
            let day = DAY_NAMES
                .iter()
                .position(|name| name.to_lowercase() == day.get(..3).unwrap_or(&day))
                .ok_or(USAGE)?;
            Repeat::Weekly {
                day: day as u8,
                at: clock(words.next())?,
            }
        }
        "every" => Repeat::Interval {
            minutes: interval(words.next().ok_or(USAGE)?).ok_or(USAGE)?,
        },
        "once" => Repeat::Once {
            at: once(clock(words.next())?, now).ok_or(USAGE)?,
        },
        _ => return Err(USAGE.into()),
    };
    let prompt = words.collect::<Vec<_>>().join(" ");
    if prompt.is_empty() {
        return Err("write what the task should do after when it runs".into());
    }
    Ok((repeat, prompt))
}

/// `2h`, `30m`, `1d` in minutes.
fn interval(word: &str) -> Option<u32> {
    let (count, unit) = word.split_at(word.find(|c: char| !c.is_ascii_digit())?);
    let count: u32 = count.parse().ok()?;
    let minutes = match unit {
        "m" | "min" => 1,
        "h" => 60,
        "d" => 24 * 60,
        _ => return None,
    };
    count.checked_mul(minutes)
}

/// The next time the clock shows `at`: today, or tomorrow once it has passed.
fn once(at: Clock, now: chrono::DateTime<chrono::Local>) -> Option<i64> {
    use chrono::TimeZone;
    let time = chrono::NaiveTime::from_hms_opt(u32::from(at.hour), u32::from(at.minute), 0)?;
    let today = now.date_naive();
    [today, today.succ_opt()?].into_iter().find_map(|date| {
        let moment = chrono::Local
            .from_local_datetime(&date.and_time(time))
            .earliest()?;
        (moment > now).then(|| moment.timestamp_millis())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::Local::now()
    }

    #[test]
    fn the_words_give_the_repeat_and_the_rest_is_the_prompt() {
        let (repeat, prompt) = parse("daily 9:30 check the build", now()).unwrap();
        assert_eq!(
            repeat,
            Repeat::Daily {
                at: Clock {
                    hour: 9,
                    minute: 30
                }
            }
        );
        assert_eq!(prompt, "check the build");
        let (repeat, _) = parse("weekly Friday 18:00 sum up", now()).unwrap();
        assert_eq!(
            repeat,
            Repeat::Weekly {
                day: 4,
                at: Clock {
                    hour: 18,
                    minute: 0
                }
            }
        );
        let (repeat, _) = parse("every 2h look", now()).unwrap();
        assert_eq!(repeat, Repeat::Interval { minutes: 120 });
    }

    #[test]
    fn broken_or_empty_specs_are_refused() {
        for text in [
            "",
            "daily",
            "daily 25:00 x",
            "weekly xyz 09:00 x",
            "every 2w x",
            "daily 09:00",
            "every 99999999999h x",
        ] {
            assert!(parse(text, now()).is_err(), "{text}");
        }
    }
}
