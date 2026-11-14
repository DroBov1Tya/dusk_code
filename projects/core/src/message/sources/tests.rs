use std::io::Cursor;

use base64::{Engine, engine::general_purpose::STANDARD};
use dusk_protocol::thread::wrap;
use image::ImageFormat;

use super::{FOLDER, ThreadSources, paths::file_paths, picture, picture::MAX_PICTURE_EDGE};
use crate::{
    catalog::mentions::Mentions,
    providers::ChatMessage,
    testing::{temp_dir, temp_state},
};

fn sounds(workspace: &std::path::Path) -> crate::state::artifacts::ArtifactStore {
    crate::state::artifacts::ArtifactStore::new(workspace.join("artifacts"))
}

#[tokio::test]
async fn an_uploaded_picture_must_be_a_small_png_or_jpeg_with_a_plain_name() {
    let state = temp_state("sources-upload").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-upload-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    assert!(
        sources
            .add_picture("x.png", b"<svg onload=x>")
            .await
            .is_err()
    );
    let mut huge = png.clone();
    huge.resize(dusk_protocol::client::MAX_PICTURE_UPLOAD + 1, 0);
    assert!(sources.add_picture("big.png", &huge).await.is_err());
    let kept = sources
        .add_picture("../../etc/shot.png", &png)
        .await
        .unwrap();
    assert_eq!(kept.name, "shot.png");
    assert!(!kept.file.contains('/'));
}

#[tokio::test]
async fn a_source_goes_once_unless_pinned_and_a_paste_leaves_the_message() {
    let state = temp_state("sources").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let message = format!("look: {}", wrap("SECRET BODY"));
    let kept = sources
        .keep_pastes(&message, &sounds(&workspace))
        .await
        .unwrap();
    assert!(!kept.contains("SECRET BODY") && kept.contains("pasted-1.txt"));

    let first = sources.block(true).await.unwrap().unwrap();
    assert!(first.text.contains("SECRET BODY"));
    assert_eq!(first.first_time.len(), 1);
    state
        .mark_sources_delivered(&first.first_time)
        .await
        .unwrap();

    let second = sources.block(true).await.unwrap().unwrap();
    assert!(!second.text.contains("SECRET BODY") && second.text.contains("pasted-1.txt"));
    assert!(second.first_time.is_empty());

    let id = state.sources("thread").await.unwrap().remove(0).source_id;
    state.pin_source("thread", &id, true).await.unwrap();
    let pinned = sources.block(true).await.unwrap().unwrap();
    assert!(pinned.text.contains("SECRET BODY"));

    sources.remove(&id).await.unwrap();
    let empty = sources.block(true).await.unwrap();
    assert!(empty.is_none());
    assert_eq!(
        std::fs::read_dir(workspace.join(FOLDER)).unwrap().count(),
        0
    );
}

#[tokio::test]
async fn a_mention_brings_its_text_once_and_unsent_files_stay_home() {
    use crate::catalog::mentions::{Profile, ProfileFile};
    let state = temp_state("sources-mentions").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-mentions-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let rules = temp_dir("sources-mentions-rules");
    let mentions = Mentions::new(&rules);
    mentions.save_snippet("video_1", "SCRIPT").unwrap();
    let files = temp_dir("sources-mentions-files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("cv.txt"), "CV").unwrap();
    std::fs::write(files.join("diary.txt"), "PRIVATE").unwrap();
    let file = |name: &str, send| ProfileFile {
        path: files.join(name),
        send,
    };
    let profile = Profile {
        about: "ABOUT".into(),
        files: vec![file("cv.txt", true), file("diary.txt", false)],
    };
    mentions.save_profile(&profile).unwrap();

    let kept = sources
        .keep_mentions("@me and @me, @video_1, mail a@video_1 @nobody", &mentions)
        .await
        .unwrap();
    assert!(kept.contains("@nobody") && kept.contains("a@video_1"));
    assert!(!kept.contains("@me") && !kept.contains(" @video_1"));
    let names: Vec<_> = state
        .sources("thread")
        .await
        .unwrap()
        .into_iter()
        .map(|source| source.name)
        .collect();
    assert_eq!(names, ["me.md", "cv.txt", "video_1.md"]);
    let block = sources.block(true).await.unwrap().unwrap();
    assert!(block.text.contains("SCRIPT") && !block.text.contains("PRIVATE"));
}

#[tokio::test]
async fn a_pasted_picture_path_becomes_a_source() {
    let state = temp_state("sources-picture-path").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-picture-path-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let dir = temp_dir("sources-picture-path-files");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = dir.join("my shot.png");
    std::fs::write(&shot, png(16)).unwrap();
    std::fs::write(dir.join("fake.png"), "not a picture").unwrap();
    let escaped = shot.display().to_string().replace(' ', "\\ ");
    let message = format!(
        "see {escaped}, and '{}' and {}/fake.png",
        shot.display(),
        dir.display()
    );
    let kept = sources
        .keep_pastes(&message, &sounds(&workspace))
        .await
        .unwrap();
    assert_eq!(
        kept,
        format!(
            "see <source>my_shot.png</source>, and <source>my_shot.png</source> and {}/fake.png",
            dir.display()
        )
    );
    assert_eq!(state.sources("thread").await.unwrap().len(), 2);
    let url = format!("file://{}", shot.display().to_string().replace(' ', "%20"));
    assert_eq!(file_paths(&url)[0].1, shot);
}

#[tokio::test]
async fn a_sent_sound_path_becomes_a_card_and_keeps_its_path() {
    let state = temp_state("sources-sound-path").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-sound-path-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let dir = temp_dir("sources-sound-path-files");
    std::fs::create_dir_all(&dir).unwrap();
    let voice = dir.join("voice.m4a");
    std::fs::write(&voice, b"\0\0\0\x20ftypM4A \0\0\0\0").unwrap();
    let fake = dir.join("fake.mp3");
    std::fs::write(&fake, "#!/bin/sh").unwrap();
    let message = format!("hear {} and {}", voice.display(), fake.display());
    let store = sounds(&workspace);
    let kept = sources.keep_pastes(&message, &store).await.unwrap();
    let parts = dusk_protocol::thread::split(&kept);
    let [_, dusk_protocol::thread::Part::Sound { id, path }, rest] = parts.as_slice() else {
        panic!("one sound in {kept}");
    };
    assert_eq!(*path, voice.display().to_string());
    let left = format!(" and {}", fake.display());
    assert!(matches!(rest, dusk_protocol::thread::Part::Text(text) if *text == left));
    let card = dusk_protocol::tools::Card {
        id: (*id).to_owned(),
        ..Default::default()
    };
    let copy = store.audio_path(&card).unwrap();
    assert_eq!(std::fs::read(copy).unwrap(), std::fs::read(&voice).unwrap());
    // Sounds are not sources: the model reads the file at its own path.
    assert!(state.sources("thread").await.unwrap().is_empty());
}

fn png(edge: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::new(edge, edge)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn a_large_picture_is_made_smaller_and_a_small_one_stays() {
    let small = picture(&png(16)).unwrap();
    assert_eq!(small.mime_type, "image/png");
    let large = picture(&png(2000)).unwrap();
    assert_eq!(large.mime_type, "image/jpeg");
    let bytes = STANDARD.decode(large.base64).unwrap();
    let shrunk = image::load_from_memory(&bytes).unwrap();
    assert_eq!(shrunk.width().max(shrunk.height()), MAX_PICTURE_EDGE);
}

#[tokio::test]
async fn a_picture_goes_to_a_model_that_takes_it_and_is_named_otherwise() {
    let state = temp_state("sources-picture").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-picture-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    sources.store("shot.png", &png(16)).await.unwrap();
    let seen = sources.block(true).await.unwrap().unwrap();
    assert_eq!(seen.pictures.len(), 1);
    assert!(matches!(
        seen.message(),
        ChatMessage::UserWithPictures { .. }
    ));
    let named = sources.block(false).await.unwrap().unwrap();
    assert!(named.pictures.is_empty() && named.text.contains("cannot see"));
    assert!(matches!(named.message(), ChatMessage::User(_)));
}
