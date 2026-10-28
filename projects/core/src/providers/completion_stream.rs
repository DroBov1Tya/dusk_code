//! Streamed Chat Completions response: SSE chunks decoded into ordered deltas.

use std::collections::VecDeque;

use anyhow::{Result, anyhow, bail, ensure};
use reqwest::Response;
use serde::Deserialize;

use super::{chat::ChatDelta, sse_decoder::SseDecoder};

const DONE_MARKER: &str = "[DONE]";

pub(super) struct CompletionStream {
    response: Response,
    parser: CompletionParser,
    pending: VecDeque<ChatDelta>,
    finished: bool,
}

impl CompletionStream {
    pub(super) fn new(response: Response) -> Self {
        Self {
            response,
            parser: CompletionParser::default(),
            pending: VecDeque::new(),
            finished: false,
        }
    }

    pub(super) async fn next_delta(&mut self) -> Result<Option<ChatDelta>> {
        loop {
            if let Some(delta) = self.pending.pop_front() {
                return Ok(Some(delta));
            }
            if self.finished {
                return Ok(None);
            }
            // Mark first so an error also ends the stream.
            self.finished = true;
            let chunk = self
                .response
                .chunk()
                .await
                .map_err(|_| anyhow!("cannot read provider stream"))?;
            match chunk {
                Some(bytes) => {
                    self.pending.extend(self.parser.feed(&bytes)?);
                    self.finished = self.parser.done;
                }
                None => self.parser.finish()?,
            }
        }
    }
}

/// Network-independent parser, so the protocol can be tested from raw bytes.
#[derive(Default)]
struct CompletionParser {
    decoder: SseDecoder,
    finish_seen: bool,
    done: bool,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    error: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    index: u32,
    delta: Option<Delta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
    /// Field name differs between servers (vLLM/LiteLLM vs. OpenRouter).
    #[serde(alias = "reasoning")]
    reasoning_content: Option<String>,
}

impl CompletionParser {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ChatDelta>> {
        let mut deltas = Vec::new();
        for data in self.decoder.feed(bytes)? {
            if self.done {
                break;
            }
            if data == DONE_MARKER {
                self.finish()?;
                break;
            }
            deltas.extend(self.parse_chunk(&data)?);
        }
        Ok(deltas)
    }

    /// The model is done once it reported a finish reason; `[DONE]` or EOF alone is a cut stream.
    fn finish(&mut self) -> Result<()> {
        self.done = true;
        ensure!(
            self.finish_seen,
            "provider stream ended before the response was complete"
        );
        Ok(())
    }

    fn parse_chunk(&mut self, data: &str) -> Result<Vec<ChatDelta>> {
        // Upstream payloads can echo input; never copy them into errors.
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|_| anyhow!("invalid provider stream chunk JSON"))?;
        if chunk.error.is_some() {
            bail!("provider reported an error during streaming");
        }
        let mut deltas = Vec::new();
        for choice in chunk.choices.into_iter().filter(|choice| choice.index == 0) {
            if let Some(delta) = choice.delta {
                deltas.extend(
                    delta
                        .reasoning_content
                        .filter(|text| !text.is_empty())
                        .map(ChatDelta::Reasoning),
                );
                deltas.extend(
                    delta
                        .content
                        .filter(|text| !text.is_empty())
                        .map(ChatDelta::Text),
                );
            }
            if let Some(reason) = choice.finish_reason {
                check_finish_reason(&reason)?;
                self.finish_seen = true;
            }
        }
        Ok(deltas)
    }
}

/// Only a natural stop is a complete answer; a cut one must not be stored as completed.
fn check_finish_reason(reason: &str) -> Result<()> {
    match reason {
        "stop" => Ok(()),
        "length" => bail!("model answer was cut by the token limit"),
        "content_filter" => bail!("model answer was blocked by the content filter"),
        _ => bail!("model stopped for an unsupported reason"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(delta: &str) -> String {
        format!("data: {{\"choices\":[{{\"index\":0,\"delta\":{delta}}}]}}\n\n")
    }

    const FINISH: &str =
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";

    #[test]
    fn splits_reasoning_and_text_in_order() {
        let body = [
            chunk(r#"{"role":"assistant","content":""}"#),
            chunk(r#"{"reasoning_content":"think"}"#),
            chunk(r#"{"content":"Hi"}"#),
            FINISH.to_owned(),
            "data: {\"choices\":[],\"usage\":{\"total_tokens\":3}}\n\n".to_owned(),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat();
        let mut parser = CompletionParser::default();
        let deltas = parser.feed(body.as_bytes()).unwrap();
        assert_eq!(
            deltas,
            [
                ChatDelta::Reasoning("think".into()),
                ChatDelta::Text("Hi".into())
            ]
        );
        assert!(parser.done);
    }

    #[test]
    fn accepts_reasoning_field_alias() {
        let mut parser = CompletionParser::default();
        let deltas = parser
            .feed(chunk(r#"{"reasoning":"r"}"#).as_bytes())
            .unwrap();
        assert_eq!(deltas, [ChatDelta::Reasoning("r".into())]);
    }

    #[test]
    fn eof_after_finish_reason_is_complete() {
        let mut parser = CompletionParser::default();
        parser.feed(FINISH.as_bytes()).unwrap();
        assert!(parser.finish().is_ok());
    }

    #[test]
    fn cut_stream_is_an_error() {
        let mut parser = CompletionParser::default();
        parser
            .feed(chunk(r#"{"content":"partial"}"#).as_bytes())
            .unwrap();
        assert!(parser.finish().is_err());
        assert!(
            CompletionParser::default()
                .feed(b"data: [DONE]\n\n")
                .is_err()
        );
    }

    #[test]
    fn truncated_answer_is_an_error() {
        let body =
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\n";
        assert!(CompletionParser::default().feed(body.as_bytes()).is_err());
    }

    #[test]
    fn ignores_data_after_done() {
        let body = format!("{FINISH}data: [DONE]\n\n{}", chunk(r#"{"content":"late"}"#));
        let mut parser = CompletionParser::default();
        assert!(parser.feed(body.as_bytes()).unwrap().is_empty());
    }

    #[test]
    fn stream_error_does_not_echo_upstream_message() {
        let body = "data: {\"error\":{\"message\":\"secret echo\"}}\n\n";
        let error = CompletionParser::default()
            .feed(body.as_bytes())
            .unwrap_err();
        assert!(!format!("{error:#}").contains("secret"));
    }
}
