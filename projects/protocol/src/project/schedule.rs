//! Scheduled tasks: a prompt the core sends, on a timetable, to a new chat in a project,
//! and how a client reads one from a form or from typed words.

use serde::{Deserialize, Serialize};

/// The most a timetable may hold, so a client cannot fill the core with timers.
pub const MAX_SCHEDULES: usize = 100;
/// The longest prompt a schedule keeps, in characters.
pub const MAX_PROMPT_CHARS: usize = 20_000;
/// The shortest interval, so a mistake cannot start a model run every minute.
pub const MIN_INTERVAL_MINUTES: u32 = 5;
/// The longest interval: a week.
pub const MAX_INTERVAL_MINUTES: u32 = 7 * 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub project_id: String,
    pub prompt: String,
    pub repeat: Repeat,
    pub enabled: bool,
    /// `provider/model` the task runs with; `None` takes the chosen one.
    #[serde(default)]
    pub model: Option<String>,
    /// Unix milliseconds; `None` once it will not run again.
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    /// The chat its last run started.
    pub last_thread_id: Option<String>,
}

/// What a client sends to make or change a schedule; the core works out when it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleDraft {
    /// `None` makes a new one.
    pub id: Option<String>,
    pub project_id: String,
    pub prompt: String,
    pub repeat: Repeat,
    pub enabled: bool,
    #[serde(default)]
    pub model: Option<String>,
}

/// A time of day on this machine's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    pub hour: u8,
    pub minute: u8,
}

/// When a schedule runs. Days are a bit set, bit 0 for Monday to bit 6 for Sunday.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Repeat {
    /// One run, at a moment in Unix milliseconds.
    Once {
        at: i64,
    },
    /// Every so many minutes, counted from the last run.
    Interval {
        minutes: u32,
    },
    Daily {
        at: Clock,
    },
    /// Monday to Friday.
    Weekdays {
        at: Clock,
    },
    /// Saturday and Sunday.
    Weekends {
        at: Clock,
    },
    /// One day a week, 0 for Monday.
    Weekly {
        day: u8,
        at: Clock,
    },
    /// The chosen days.
    Custom {
        days: u8,
        at: Clock,
    },
}

pub const WEEKDAYS: u8 = 0b001_1111;
pub const WEEKENDS: u8 = 0b110_0000;
pub const EVERY_DAY: u8 = 0b111_1111;

/// Short day names, Monday first, as the day bits count them.
pub const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl Clock {
    /// Reads `9:30` or `09:30`.
    pub fn parse(text: &str) -> Option<Clock> {
        let (hour, minute) = text.trim().split_once(':')?;
        let (hour, minute) = (hour.trim().parse().ok()?, minute.trim().parse().ok()?);
        (hour < 24 && minute < 60).then_some(Clock { hour, minute })
    }
}

impl std::fmt::Display for Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

/// "15 minutes", "1 hour", "2 hours", "1 day".
pub fn interval_name(minutes: u32) -> String {
    let (count, unit) = match minutes {
        m if m % (24 * 60) == 0 => (m / (24 * 60), "day"),
        m if m % 60 == 0 => (m / 60, "hour"),
        m => (m, "minute"),
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

impl Repeat {
    /// How it repeats, in a few words; a single run says only "Once", as its moment is
    /// shown on the client's clock.
    pub fn describe(&self) -> String {
        match *self {
            Repeat::Once { .. } => "Once".to_owned(),
            Repeat::Interval { minutes } => format!("Every {}", interval_name(minutes)),
            Repeat::Daily { at } => format!("Every day at {at}"),
            Repeat::Weekdays { at } => format!("Weekdays at {at}"),
            Repeat::Weekends { at } => format!("Weekends at {at}"),
            Repeat::Weekly { day, at } => format!(
                "Every {} at {at}",
                DAY_NAMES.get(usize::from(day)).copied().unwrap_or("?")
            ),
            Repeat::Custom { days, at } => {
                let names: Vec<&str> = DAY_NAMES
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| days & (1 << index) != 0)
                    .map(|(_, name)| *name)
                    .collect();
                format!("{} at {at}", names.join(", "))
            }
        }
    }

    /// The days it runs on and the time, or `None` for one that is not tied to days.
    pub fn days_and_time(&self) -> Option<(u8, Clock)> {
        match *self {
            Repeat::Once { .. } | Repeat::Interval { .. } => None,
            Repeat::Daily { at } => Some((EVERY_DAY, at)),
            Repeat::Weekdays { at } => Some((WEEKDAYS, at)),
            Repeat::Weekends { at } => Some((WEEKENDS, at)),
            Repeat::Weekly { day, at } => Some((1u8.checked_shl(u32::from(day))?, at)),
            Repeat::Custom { days, at } => Some((days, at)),
        }
    }
}

/// How a task repeats, without its times: what a schedule form chooses first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatKind {
    Once,
    Interval,
    Daily,
    Weekdays,
    Weekends,
    Weekly,
    Custom,
}

impl RepeatKind {
    /// The repeat a form makes: `time` as typed, the interval, day and days chosen, and
    /// `once` placing a single run's time on the client's clock in Unix milliseconds.
    pub fn repeat(
        self,
        time: &str,
        interval: u32,
        day: u8,
        days: u8,
        once: impl FnOnce(Clock) -> Result<i64, String>,
    ) -> Result<Repeat, String> {
        let at = || Clock::parse(time).ok_or_else(|| "write the time as 09:30".to_owned());
        Ok(match self {
            RepeatKind::Once => Repeat::Once { at: once(at()?)? },
            RepeatKind::Interval => Repeat::Interval { minutes: interval },
            RepeatKind::Daily => Repeat::Daily { at: at()? },
            RepeatKind::Weekdays => Repeat::Weekdays { at: at()? },
            RepeatKind::Weekends => Repeat::Weekends { at: at()? },
            RepeatKind::Weekly => Repeat::Weekly { day, at: at()? },
            RepeatKind::Custom => Repeat::Custom { days, at: at()? },
        })
    }
}

/// When a typed schedule runs. A single run is a time of day; the client puts it on its
/// clock, today or tomorrow, since only the client knows its time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    Repeat(Repeat),
    Once(Clock),
}

/// How a schedule is typed, said when the words cannot be read.
pub const SCHEDULE_USAGE: &str = "write /schedule daily 09:00 <prompt>; also weekdays, \
                                  weekends, weekly mon 09:00, every 2h, every 30m, once 14:30";

/// Reads `daily 09:00`, `weekdays 09:00`, `weekends 10:30`, `weekly mon 09:00`, `every 2h`,
/// `every 30m` or `once 14:30`, then the prompt.
pub fn parse_schedule(text: &str) -> Result<(When, String), String> {
    let usage = || SCHEDULE_USAGE.to_owned();
    let mut words = text.split_whitespace();
    let kind = words.next().ok_or_else(usage)?.to_lowercase();
    let clock = |word: Option<&str>| word.and_then(Clock::parse).ok_or_else(usage);
    let when = match kind.as_str() {
        "daily" => When::Repeat(Repeat::Daily {
            at: clock(words.next())?,
        }),
        "weekdays" => When::Repeat(Repeat::Weekdays {
            at: clock(words.next())?,
        }),
        "weekends" => When::Repeat(Repeat::Weekends {
            at: clock(words.next())?,
        }),
        "weekly" => {
            let day = words.next().ok_or_else(usage)?.to_lowercase();
            let day = DAY_NAMES
                .iter()
                .position(|name| name.to_lowercase() == day.get(..3).unwrap_or(&day))
                .ok_or_else(usage)?;
            When::Repeat(Repeat::Weekly {
                day: day as u8,
                at: clock(words.next())?,
            })
        }
        "every" => When::Repeat(Repeat::Interval {
            minutes: parse_interval(words.next().ok_or_else(usage)?).ok_or_else(usage)?,
        }),
        "once" => When::Once(clock(words.next())?),
        _ => return Err(usage()),
    };
    let prompt = words.collect::<Vec<_>>().join(" ");
    if prompt.is_empty() {
        return Err("write what the task should do after when it runs".into());
    }
    Ok((when, prompt))
}

/// `2h`, `30m`, `1d` in minutes.
fn parse_interval(word: &str) -> Option<u32> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(hour: u8, minute: u8) -> Clock {
        Clock { hour, minute }
    }

    #[test]
    fn the_words_give_the_repeat_and_the_rest_is_the_prompt() {
        let (when, prompt) = parse_schedule("daily 9:30 check the build").unwrap();
        assert_eq!(when, When::Repeat(Repeat::Daily { at: clock(9, 30) }));
        assert_eq!(prompt, "check the build");
        let (when, _) = parse_schedule("weekly Friday 18:00 sum up").unwrap();
        assert_eq!(
            when,
            When::Repeat(Repeat::Weekly {
                day: 4,
                at: clock(18, 0)
            })
        );
        let (when, _) = parse_schedule("every 2h look").unwrap();
        assert_eq!(when, When::Repeat(Repeat::Interval { minutes: 120 }));
        let (when, _) = parse_schedule("ONCE 14:30 ship").unwrap();
        assert_eq!(when, When::Once(clock(14, 30)));
    }

    #[test]
    fn broken_or_empty_specs_are_refused() {
        for text in [
            "",
            "daily",
            "daily 25:00 x",
            "daily 9:60 x",
            "weekly xyz 09:00 x",
            "weekly",
            "every 2w x",
            "every h x",
            "every -2h x",
            "once x",
            "daily 09:00",
            "every 99999999999h x",
            "every 9999999d x",
            "monthly 09:00 x",
        ] {
            assert!(parse_schedule(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_form_needs_a_time_only_where_the_kind_uses_one() {
        let never = |_| -> Result<i64, String> { unreachable!() };
        assert_eq!(
            RepeatKind::Interval.repeat("", 30, 0, 0, never),
            Ok(Repeat::Interval { minutes: 30 })
        );
        assert!(RepeatKind::Daily.repeat("9", 0, 0, 0, never).is_err());
        assert_eq!(
            RepeatKind::Custom.repeat("07:05", 0, 0, WEEKENDS, never),
            Ok(Repeat::Custom {
                days: WEEKENDS,
                at: clock(7, 5)
            })
        );
        assert_eq!(
            RepeatKind::Once.repeat("10:00", 0, 0, 0, |at| Ok(i64::from(at.hour))),
            Ok(Repeat::Once { at: 10 })
        );
        assert_eq!(
            RepeatKind::Once.repeat("10:00", 0, 0, 0, |_| Err("choose a date".into())),
            Err("choose a date".into())
        );
    }
}
