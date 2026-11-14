use std::sync::Arc;

use dusk_protocol::config::{LocalState, SpeechKind, VoiceEndpoint};

use super::{LocalVoice, catalog::MODELS, run::is_ready, uv};
use crate::{config::live::LiveConfig, secrets::MemoryStore, testing::temp_dir};

fn local_voice(name: &str) -> (LocalVoice, std::path::PathBuf) {
    let dir = temp_dir(name);
    std::fs::create_dir_all(&dir).unwrap();
    crate::init::files::write_private(&dir.join("config.yml"), b"providers: {}\n").unwrap();
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
    let settings = dusk_protocol::config::VoiceSettings {
        tts: Some(VoiceEndpoint {
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "kokoro-82m".into(),
            voice: None,
        }),
        ..Default::default()
    };
    let listed = voice.list(&settings).await;
    let used: Vec<_> = listed.models.iter().filter(|model| model.in_use).collect();
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].kind, SpeechKind::Synthesis);
}

#[test]
fn choosing_a_model_drops_the_other_of_its_kind_only() {
    let (voice, _dir) = local_voice("voice-local-choose");
    let first = |kind| MODELS.iter().find(|model| model.kind == kind).unwrap();
    let (stt, tts) = (first(SpeechKind::Recognition), first(SpeechKind::Synthesis));
    voice.choose(tts).unwrap();
    voice.choose(stt).unwrap();
    assert_eq!(voice.chosen().len(), 2);
    voice.forget(stt).unwrap();
    assert_eq!(voice.chosen().into_iter().collect::<Vec<_>>(), [tts.id]);
}

#[tokio::test]
async fn a_cancelled_install_leaves_nothing_but_an_installed_model_stays() {
    let (voice, _dir) = local_voice("voice-local-cancel");
    assert!(voice.cancel("../x").await.is_err());
    let model = &MODELS[0];
    // A half-made install: some files, no marker.
    std::fs::create_dir_all(voice.dir(model).join("venv")).unwrap();
    voice.stage(model, "Installing packages");
    voice.cancel(model.id).await.unwrap();
    assert!(!voice.dir(model).exists());
    let listed = voice.list(&Default::default()).await;
    assert_eq!(listed.models[0].state, LocalState::NotInstalled);
    // One that was installed before, and is only being started again.
    std::fs::create_dir_all(voice.dir(model)).unwrap();
    std::fs::write(voice.dir(model).join(super::install::READY), b"1").unwrap();
    voice.stage(model, "Starting the server");
    voice.cancel(model.id).await.unwrap();
    assert!(voice.installed(model));
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

/// The whole road with the real thing: `uv`, the packages, the weights, the server and the
/// speech clients. Slow and large, so it runs on request:
/// `cargo test -p dusk_core local_models_talk -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "installs real models with uv: needs the network and about 3 GB"]
async fn local_models_talk_to_each_other() {
    use dusk_protocol::config::{STT_KEY, TTS_KEY};
    let (voice, _dir) = local_voice("voice-local-real");
    let voice = Arc::new(voice);
    let ids = chosen_for_the_run();
    for id in &ids {
        let model = super::find(id).unwrap();
        voice.install(model.id).await.unwrap();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let listed = voice.list(&Default::default()).await;
            let state = &listed
                .models
                .iter()
                .find(|m| m.id == model.id)
                .unwrap()
                .state;
            println!("{}: {state:?}", model.id);
            match state {
                LocalState::Installing(_) => {}
                LocalState::Running => break,
                other => panic!("{other:?}"),
            }
        }
    }
    let settings = voice.config.current().voice().clone();
    let store = voice.config.secrets().unwrap().clone();
    let (stt, tts) = (settings.stt.unwrap(), settings.tts.unwrap());
    let said = "Hello from the local voice server.";
    let sound = crate::voice::speak(&tts, store.get(TTS_KEY).unwrap().as_deref(), said)
        .await
        .unwrap();
    assert!(sound.starts_with(b"RIFF"));
    let text = crate::voice::transcribe(&stt, store.get(STT_KEY).unwrap().as_deref(), &sound)
        .await
        .unwrap();
    println!("heard: {text}");
    assert!(text.to_lowercase().contains("hello"));
    for id in &ids {
        let model = super::find(id).unwrap();
        voice.remove(model.id).await.unwrap();
        assert!(!voice.dir(model).exists());
    }
    let after = voice.config.current().voice().clone();
    assert!(after.stt.is_none() && after.tts.is_none());
    assert!(voice.chosen().is_empty());
}

/// The models the long test installs: `DUSK_VOICE_TEST` names two ids, else the first of
/// each kind.
fn chosen_for_the_run() -> Vec<String> {
    if let Ok(ids) = std::env::var("DUSK_VOICE_TEST") {
        return ids.split(',').map(str::to_owned).collect();
    }
    [SpeechKind::Recognition, SpeechKind::Synthesis]
        .map(|kind| {
            MODELS
                .iter()
                .find(|model| model.kind == kind)
                .unwrap()
                .id
                .to_owned()
        })
        .to_vec()
}
