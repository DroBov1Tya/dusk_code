//! Commands a client sends as a message: a known `/word` at the very start, optionally
//! followed by an argument. Anything else is an ordinary message, `/etc/hosts` included.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Summarizes the context so far; the argument says what the summary should focus on.
    Compact { focus: Option<String> },
}

/// Every command, for clients to show: name and what it does.
pub const COMMANDS: &[(&str, &str)] = &[(
    "/compact",
    "summarize the conversation so far to free the context; an argument sets the focus",
)];

/// `None` for an ordinary message; an error for a `/word` that is not a command.
pub fn parse(text: &str) -> Option<Result<Command>> {
    let text = text.trim_start();
    let rest = text.strip_prefix('/')?;
    let (name, argument) = match rest.split_once(char::is_whitespace) {
        Some((name, argument)) => (name, argument.trim()),
        None => (rest, ""),
    };
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        return None;
    }
    let argument = (!argument.is_empty()).then(|| argument.to_owned());
    Some(match name {
        "compact" => Ok(Command::Compact { focus: argument }),
        _ => unknown(name),
    })
}

fn unknown(name: &str) -> Result<Command> {
    bail!("unknown command /{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_command_and_its_argument() {
        let compact = |focus: Option<&str>| {
            Some(Command::Compact {
                focus: focus.map(Into::into),
            })
        };
        assert_eq!(parse("/compact").and_then(Result::ok), compact(None));
        assert_eq!(parse("  /compact  ").and_then(Result::ok), compact(None));
        assert_eq!(
            parse("/compact the API design\nand tests").and_then(Result::ok),
            compact(Some("the API design\nand tests"))
        );
    }

    #[test]
    fn ordinary_text_is_not_a_command() {
        for text in [
            "hello /compact",
            "/etc/hosts is empty",
            "/",
            "/ compact",
            "/Compact",
        ] {
            assert!(parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn an_unknown_command_is_an_error() {
        let error = parse("/compactt now").unwrap().unwrap_err();
        assert_eq!(error.to_string(), "unknown command /compactt");
    }
}
