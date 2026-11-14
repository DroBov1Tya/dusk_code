//! `/schedule <when> <prompt>`: the protocol reads the words; a single run (`once 14:30`)
//! is placed here on this machine's clock, today or tomorrow once that time has passed.

use dusk_protocol::project::{Clock, Repeat, SCHEDULE_USAGE, When, parse_schedule};

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

/// The repeat and the prompt from what follows `/schedule `; `now` places a single run.
pub fn parse(text: &str, now: chrono::DateTime<chrono::Local>) -> Result<(Repeat, String), String> {
    let (when, prompt) = parse_schedule(text)?;
    let repeat = match when {
        When::Repeat(repeat) => repeat,
        When::Once(at) => Repeat::Once {
            at: once(at, now).ok_or(SCHEDULE_USAGE)?,
        },
    };
    Ok((repeat, prompt))
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
