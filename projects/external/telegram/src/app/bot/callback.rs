//! Callback data of inline buttons.
//!
//! Callback data comes from the client and can be forged, so it is parsed into this
//! enum before any handler sees it. Every button carries the nonce of the run that sent
//! it, so a button left from an earlier run answers nothing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callback {
    /// The single answer to the open question, by option.
    Choose(usize),
    /// One of several answers, marked or unmarked until Send.
    Toggle(usize),
    Send,
    Allow(u64),
    Deny(u64),
}

impl Callback {
    pub fn data(self, nonce: &str) -> String {
        let body = match self {
            Self::Choose(index) => format!("c:{index}"),
            Self::Toggle(index) => format!("t:{index}"),
            Self::Send => "s".into(),
            Self::Allow(id) => format!("y:{id}"),
            Self::Deny(id) => format!("n:{id}"),
        };
        format!("{nonce}|{body}")
    }

    /// `None` for data of another run, or that this bot never sends.
    pub fn parse(data: &str, nonce: &str) -> Option<Self> {
        let (sent, body) = data.split_once('|')?;
        if sent != nonce {
            return None;
        }
        let (kind, value) = body.split_once(':').unwrap_or((body, ""));
        Some(match kind {
            "c" => Self::Choose(value.parse().ok()?),
            "t" => Self::Toggle(value.parse().ok()?),
            "s" if value.is_empty() => Self::Send,
            "y" => Self::Allow(value.parse().ok()?),
            "n" => Self::Deny(value.parse().ok()?),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_of_this_run_round_trips_and_other_data_is_refused() {
        for callback in [
            Callback::Choose(2),
            Callback::Toggle(0),
            Callback::Send,
            Callback::Allow(7),
            Callback::Deny(7),
        ] {
            assert_eq!(
                Callback::parse(&callback.data("123"), "123"),
                Some(callback)
            );
            assert_eq!(Callback::parse(&callback.data("999"), "123"), None);
        }
        for forged in ["y:1", "123|x:1", "123|c:-1", "123|s:1", "123|y:"] {
            assert_eq!(Callback::parse(forged, "123"), None, "{forged}");
        }
    }
}
