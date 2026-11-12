//! Scheduled tasks: a prompt the core sends, on a timetable, to a new chat in a project.

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
