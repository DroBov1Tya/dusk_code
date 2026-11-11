use std::sync::Arc;

use dusk_protocol::config::{LocalState, SpeechKind, VoiceEndpoint};

use super::{LocalVoice, catalog::MODELS, run::is_ready, uv};
use crate::{config::live::LiveConfig, secrets::MemoryStore, testing::temp_dir};

fn local_voice(name: &str) -> (LocalVoice, std::path::PathBuf) {
    let dir = temp_dir(name);
    std::fs::create_dir_all(&dir).unwrap();
    crate::util::atomic::write_private(&dir.join("config.yml"), b"providers: {}\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let config = LiveConfig::load_with(
        dir.join("config.yml"),
        Some(Arc::new(MemoryStore::default())),
    )
    .unwrap();
    (LocalVoice::new(dir.join("voice"), Arc::new(config)), dir)
}

#[tokio::test]
async fn only_the_models_of_the_catalog_are_installed_or_removed() {
    let (voice, dir) = local_voice("voice-local-names");
    let voice = Arc::new(voice);
    for id in [
        "",
        "../escape",
        "/etc",
        "whisper-large-v3-turbo/../x",
        "unknown",
    ] {
        assert!(voice.install(id).await.is_err(), "{id}");
        assert!(voice.remove(id).await.is_err(), "{id}");
    }
    assert!(!dir.join("voice").exists());
}

#[tokio::test]
async fn a_model_that_is_not_installed_is_listed_as_such() {
    let (voice, _dir) = local_voice("voice-local-list");
    let listed = voice.list(&Default::default()).await;
    assert_eq!(listed.models.len(), MODELS.len());
    assert!(
        listed
            .models
            .iter()
            .all(|model| model.state == LocalState::NotInstalled && !model.in_use)
    );
}

#[tokio::test]
async fn a_model_the_settings_point_at_is_in_use() {
    let (voice, _dir) = local_voice("voice-local-use");
    let mut settings = dusk_protocol::config::VoiceSettings::default();
    settings.tts = Some(VoiceEndpoint {
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "kokoro-82m".into(),
        voice: None,
    });
    let listed = voice.list(&settings).await;
    let used: Vec<_> = listed.models.iter().filter(|model| model.in_use).collect();
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].kind, SpeechKind::Synthesis);
}

#[test]
fn choosing_a_model_drops_the_other_of_its_kind_only() {
    let (voice, _dir) = local_voice("voice-local-choose");
    let (stt, tts) = (&MODELS[0], &MODELS[1]);
    voice.choose(tts).unwrap();
    voice.choose(stt).unwrap();
    assert_eq!(voice.chosen().len(), 2);
    voice.forget(stt).unwrap();
    assert_eq!(voice.chosen().into_iter().collect::<Vec<_>>(), [tts.id]);
}

#[test]
fn the_server_is_ready_only_by_its_own_line() {
    assert!(is_ready(r#"{"stage":"ready","port":8000}"#));
    assert!(!is_ready(r#"{"stage":"download"}"#));
    assert!(!is_ready("ready"));
    assert!(!is_ready(r#"{"stage":"ready"#));
}

#[test]
fn the_tail_of_a_failure_keeps_whole_characters() {
    let said = "ошибка ".repeat(400);
    let tail = uv::tail(said.as_bytes());
    assert!(tail.chars().count() <= 602);
    assert!(said.contains(tail.trim()));
    assert_eq!(uv::tail(b"  short \n"), "short");
}
