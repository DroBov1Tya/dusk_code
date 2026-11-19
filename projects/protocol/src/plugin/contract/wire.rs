//! The lines of one call: the core writes one `call` and closes stdin; the program
//! writes `log` lines and then one `result`, one JSON object per line, UTF-8.

use serde::{Deserialize, Serialize};

/// From the core, once per process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "call")]
pub struct Call {
    pub contract: String,
    /// The highest version both the manifest and the core speak.
    pub version: u32,
    pub op: String,
    /// The one project folder the call is about; also the working directory.
    pub folder: String,
    pub args: serde_json::Value,
}

/// From the program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromProgram {
    Result(Answer),
    /// A line for the debug log; the core caps it.
    Log {
        text: String,
    },
}

/// The body of a `result`: the answer in its operation's shape, or an error.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    #[serde(default)]
    pub ok: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<ContractError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractError {
    pub kind: ErrorKind,
    #[serde(default)]
    pub message: String,
    /// Plain, one line, at most 120 characters: what the user may type to fix it.
    #[serde(default)]
    pub hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    SignedOut,
    NotFound,
    Unavailable,
    Refused,
    Unsupported,
    Failed,
    /// A kind a newer program uses; read as `failed`.
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_line_carries_its_tag() {
        let call = Call {
            contract: "forge".into(),
            version: 1,
            op: "pull".into(),
            folder: "/p".into(),
            args: serde_json::json!({"number": 3}),
        };
        let line = serde_json::to_string(&call).unwrap();
        assert!(line.contains(r#""type":"call""#), "{line}");
        assert_eq!(serde_json::from_str::<Call>(&line).unwrap(), call);
    }

    #[test]
    fn program_lines_parse_and_unknown_kinds_fall_back() {
        let ok: FromProgram =
            serde_json::from_str(r#"{"type":"result","ok":{"a":1},"x":2}"#).unwrap();
        assert!(matches!(
            ok,
            FromProgram::Result(Answer {
                ok: Some(_),
                error: None
            })
        ));
        let err: FromProgram = serde_json::from_str(
            r#"{"type":"result","error":{"kind":"signed_out","hint":"gh auth login"}}"#,
        )
        .unwrap();
        let FromProgram::Result(Answer {
            error: Some(error), ..
        }) = err
        else {
            panic!("no error");
        };
        assert_eq!(error.kind, ErrorKind::SignedOut);
        assert_eq!(error.hint, "gh auth login");
        let odd: FromProgram =
            serde_json::from_str(r#"{"type":"result","error":{"kind":"rate_limited"}}"#).unwrap();
        let FromProgram::Result(Answer {
            error: Some(error), ..
        }) = odd
        else {
            panic!("no error");
        };
        assert_eq!(error.kind, ErrorKind::Other);
        let log: FromProgram = serde_json::from_str(r#"{"type":"log","text":"hi"}"#).unwrap();
        assert_eq!(log, FromProgram::Log { text: "hi".into() });
    }
}
