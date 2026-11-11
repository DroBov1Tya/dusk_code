//! Recordings in, speech out.

use super::Server;
use anyhow::Result;
use dusk_protocol::client::Response;

impl Server {
    pub(super) async fn voice(
        &self,
        request: dusk_protocol::client::VoiceRequest,
    ) -> Result<Response> {
        use dusk_protocol::client::VoiceRequest;
        Ok(match request {
            VoiceRequest::Transcribe { audio } => {
                Response::Text(self.runtime.transcribe(&audio).await?)
            }
            VoiceRequest::Speak { text } => Response::Card(self.runtime.speak(&text).await?),
        })
    }
}
