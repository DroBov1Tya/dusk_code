use std::io::Cursor;

use base64::{Engine, engine::general_purpose::STANDARD};
use dusk_protocol::thread::wrap;
use image::ImageFormat;

use super::{
    FOLDER, ThreadSources,
    paths::file_paths,
    picture,
    picture::{Kept, MAX_PICTURE_EDGE, PictureKey},
};
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

fn gray_png(edge: u32, shade: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(edge, edge, image::Rgb([shade, shade / 2, 255 - shade]))
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

#[tokio::test]
async fn a_picture_made_smaller_is_kept_and_sent_the_same_until_its_file_changes() {
    let state = temp_state("sources-kept-picture").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-kept-picture-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let first = gray_png(2000, 40);
    let source = sources.store("shot.png", &first).await.unwrap();
    let made = picture(&first).unwrap();
    let path = workspace.join(FOLDER).join(&source.file);
    let key = PictureKey::of(&path).await.unwrap();

    let once = sources.block(true).await.unwrap().unwrap();
    assert_eq!(once.pictures, vec![made.clone()]);
    assert_eq!(super::picture::KEPT.get(&key), Some(made.clone()));
    let again = sources.block(true).await.unwrap().unwrap();
    assert_eq!(again.pictures, vec![made.clone()]);
    assert_eq!(again.text, once.text);

    // The kept picture is what goes, not one made again: a marker put in its place
    // comes back.
    let marker = crate::providers::Picture {
        mime_type: "image/png",
        base64: "marker".into(),
    };
    super::picture::KEPT.put(key.clone(), marker.clone());
    let kept = sources.block(true).await.unwrap().unwrap();
    assert_eq!(kept.pictures, vec![marker]);
    assert_eq!(kept.text, once.text);

    // Another picture in the same file is made anew.
    let second = gray_png(1900, 200);
    std::fs::write(&path, &second).unwrap();
    let changed = sources.block(true).await.unwrap().unwrap();
    assert_eq!(changed.pictures, vec![picture(&second).unwrap()]);
    assert_ne!(changed.pictures[0], made);
}

/// A copy that keeps the time (`cp -p`, `touch -r`), or a rewrite within a coarse clock's
/// tick, leaves the length and the time of change as they were.
#[cfg(unix)]
#[tokio::test]
async fn a_picture_replaced_with_the_same_length_and_time_is_made_again() {
    let state = temp_state("sources-same-time-picture").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-same-time-picture-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let (first, second) = (gray_png(64, 40), gray_png(64, 200));
    assert_eq!(
        first.len(),
        second.len(),
        "the test needs pictures of one length"
    );
    assert_ne!(first, second);
    let source = sources.store("shot.png", &first).await.unwrap();
    let path = workspace.join(FOLDER).join(&source.file);
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let once = sources.block(true).await.unwrap().unwrap();
    assert_eq!(once.pictures, vec![picture(&first).unwrap()]);

    // Rewritten in place, and by a rename over it, each time with the old time back.
    std::fs::write(&path, &second).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    let rewritten = sources.block(true).await.unwrap().unwrap();
    assert_eq!(rewritten.pictures, vec![picture(&second).unwrap()]);

    let beside = path.with_extension("new");
    std::fs::write(&beside, &first).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&beside)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    std::fs::rename(&beside, &path).unwrap();
    let renamed = sources.block(true).await.unwrap().unwrap();
    assert_eq!(renamed.pictures, vec![picture(&first).unwrap()]);
}

#[tokio::test]
async fn kept_pictures_drop_the_least_used_past_their_bounds() {
    let dir = temp_dir("sources-kept-bounds");
    std::fs::create_dir_all(&dir).unwrap();
    let mut keys = Vec::new();
    for at in 0..3 {
        let path = dir.join(format!("{at}.png"));
        std::fs::write(&path, png(4)).unwrap();
        keys.push(PictureKey::of(&path).await.unwrap());
    }
    let shown = |text: &str| crate::providers::Picture {
        mime_type: "image/png",
        base64: text.into(),
    };
    let kept = Kept::new(2, 10);
    kept.put(keys[0].clone(), shown("aaaa"));
    kept.put(keys[1].clone(), shown("bbbb"));
    // Using the first makes the second the one to drop.
    assert_eq!(kept.get(&keys[0]), Some(shown("aaaa")));
    kept.put(keys[2].clone(), shown("cccc"));
    assert_eq!(kept.get(&keys[1]), None);
    assert_eq!(kept.get(&keys[0]), Some(shown("aaaa")));
    assert_eq!(kept.get(&keys[2]), Some(shown("cccc")));
    // Past the byte bound, the oldest goes; one too large is never kept.
    kept.put(keys[1].clone(), shown("bbbbbbbb"));
    assert_eq!(kept.get(&keys[0]), None);
    assert_eq!(kept.get(&keys[2]), None);
    kept.put(keys[0].clone(), shown("a".repeat(11).as_str()));
    assert_eq!(kept.get(&keys[0]), None);
    assert_eq!(kept.get(&keys[1]), Some(shown("bbbbbbbb")));
    // A file with another length is another state of it.
    std::fs::write(dir.join("1.png"), png(8)).unwrap();
    let changed = PictureKey::of(&dir.join("1.png")).await.unwrap();
    assert_eq!(kept.get(&changed), None);
}

#[tokio::test]
async fn a_client_reads_only_a_picture_source_of_its_chat_by_name() {
    let state = temp_state("sources-read-picture").await;
    state.create_thread("thread", "test").await.unwrap();
    let workspace = temp_dir("sources-read-picture-workspace");
    let sources = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    sources.add_picture("shot.png", &png).await.unwrap();
    let pasted = sources
        .keep_pastes(&wrap("plain text"), &sounds(&workspace))
        .await
        .unwrap();
    assert!(pasted.contains("pasted-2.txt"));
    assert_eq!(sources.picture("shot.png").await.unwrap(), png);
    assert!(sources.picture("pasted-2.txt").await.is_err());
    assert!(sources.picture("../state.db").await.is_err());
    assert!(sources.picture("nothing.png").await.is_err());
}

#[tokio::test]
async fn a_reader_gets_only_this_chats_sources_by_id() {
    let state = temp_state("sources-reader").await;
    state.create_thread("thread", "test").await.unwrap();
    state.create_thread("other", "test").await.unwrap();
    let workspace = temp_dir("sources-reader-workspace");
    let mine = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "thread",
    };
    let theirs = ThreadSources {
        state: &state,
        workspace: &workspace,
        thread_id: "other",
    };
    let source = mine.store("notes.md", b"# notes").await.unwrap();
    let chunk = mine.chunk(&source.source_id, 0).await.unwrap();
    assert_eq!(chunk.text, "# notes");
    assert_eq!(chunk.next, None);
    assert!(theirs.chunk(&source.source_id, 0).await.is_err());
    assert!(mine.chunk("../notes.md", 0).await.is_err());
}
