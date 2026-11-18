use super::*;

const A: &str = "/work/a";
const B: &str = "/work/b";

async fn state_with_projects() -> State {
    let state = State::in_memory().await.unwrap();
    state.create_project("a", "A", "", A).await.unwrap();
    state.create_project("b", "B", "", B).await.unwrap();
    state
}

fn episode(project: &str, n: u32, problem: &str) -> NewEpisode {
    NewEpisode {
        project: project.into(),
        thread_id: None,
        turn_id: None,
        kind: EpisodeKind::Fixed,
        signature: format!("{n:064x}"),
        tool: "run_command".into(),
        model: "model".into(),
        problem: problem.into(),
        failed: String::new(),
        worked: "cargo build".into(),
        lesson: String::new(),
    }
}

/// The rowids the search index holds for `word`, read from the index itself.
async fn indexed(state: &State, word: &str) -> Vec<i64> {
    sqlx::query_scalar("SELECT rowid FROM episodes_search WHERE episodes_search MATCH ?")
        .bind(format!("\"{word}\""))
        .fetch_all(&state.pool)
        .await
        .unwrap()
}

/// Fails when the index and the episodes table disagree.
async fn index_is_consistent(state: &State) {
    sqlx::query(
        "INSERT INTO episodes_search (episodes_search, rank) VALUES ('integrity-check', 1)",
    )
    .execute(&state.pool)
    .await
    .unwrap();
}

/// Whether the index has lost the episode whose problem holds `e{n:04}q`.
async fn gone(state: &State, n: usize) -> bool {
    indexed(state, &format!("e{n:04}q")).await.is_empty()
}

async fn found(state: &State, project: &str, query: &str) -> Vec<Episode> {
    match state.recall_episodes(project, query).await.unwrap() {
        Recall::Found(found) => found,
        Recall::NoUsableWord => panic!("{query:?} had no usable word"),
    }
}

#[tokio::test]
async fn fts5_is_compiled_in() {
    let state = State::in_memory().await.unwrap();
    let used: i64 = sqlx::query_scalar("SELECT sqlite_compileoption_used('ENABLE_FTS5')")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(used, 1);
}

#[tokio::test]
async fn the_same_episode_is_counted_not_copied() {
    let state = state_with_projects().await;
    let mut first = episode(A, 1, "linker cc not found");
    first.worked = "worked-alpha".into();
    first.lesson = "lesson-one".into();
    let id = state.record_episode(&first).await.unwrap();

    let mut again = first.clone();
    again.worked = "worked-beta".into();
    again.lesson = String::new();
    assert_eq!(state.record_episode(&again).await.unwrap(), id);
    let listed = state.list_episodes(A, None, 10).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].seen, 2);
    assert_eq!(listed[0].worked, "worked-beta");
    assert_eq!(listed[0].lesson, "lesson-one");
    assert!(listed[0].seen_at >= listed[0].created_at);
    // The index follows the newer text.
    assert!(indexed(&state, "worked-alpha").await.is_empty());
    assert_eq!(indexed(&state, "worked-beta").await, [id]);

    again.lesson = "lesson-two".into();
    state.record_episode(&again).await.unwrap();
    let listed = state.list_episodes(A, None, 10).await.unwrap();
    assert_eq!(
        (listed[0].seen, listed[0].lesson.as_str()),
        (3, "lesson-two")
    );

    // The same signature in another project is another episode.
    let mut other = first.clone();
    other.project = B.into();
    assert_ne!(state.record_episode(&other).await.unwrap(), id);
    index_is_consistent(&state).await;
}

#[tokio::test]
async fn a_full_project_drops_the_least_seen_oldest_first() {
    let state = state_with_projects().await;
    state
        .record_episode(&episode(B, 1, "untouched b0001q"))
        .await
        .unwrap();
    let mut ids = Vec::new();
    for n in 1..=500 {
        let problem = format!("entry e{n:04}q");
        ids.push(
            state
                .record_episode(&episode(A, n, &problem))
                .await
                .unwrap(),
        );
    }
    let set = |id: i64, seen: i64, seen_at: i64| {
        sqlx::query("UPDATE episodes SET seen = ?, seen_at = ? WHERE id = ?")
            .bind(seen)
            .bind(seen_at)
            .bind(id)
    };
    sqlx::query("UPDATE episodes SET seen = 2, created_at = 0, seen_at = 1000")
        .execute(&state.pool)
        .await
        .unwrap();
    set(ids[9], 1, 500).execute(&state.pool).await.unwrap();
    set(ids[19], 1, 200).execute(&state.pool).await.unwrap();
    // Older than both, but seen more often.
    set(ids[29], 3, 1).execute(&state.pool).await.unwrap();

    state
        .record_episode(&episode(A, 501, "entry e0501q"))
        .await
        .unwrap();
    assert_eq!(state.episode_counts().await.unwrap()[A], 500);
    assert!(
        gone(&state, 20).await,
        "the oldest of the least seen goes first"
    );
    assert!(!gone(&state, 10).await && !gone(&state, 30).await && !gone(&state, 501).await);

    state
        .record_episode(&episode(A, 502, "entry e0502q"))
        .await
        .unwrap();
    assert!(gone(&state, 10).await);

    // Every other episode seen more often: the new one still stays.
    sqlx::query("UPDATE episodes SET seen = 5")
        .execute(&state.pool)
        .await
        .unwrap();
    state
        .record_episode(&episode(A, 503, "entry e0503q"))
        .await
        .unwrap();
    assert!(!gone(&state, 503).await);
    assert_eq!(state.episode_counts().await.unwrap()[A], 500);
    assert_eq!(state.episode_counts().await.unwrap()[B], 1);
    index_is_consistent(&state).await;
}

#[tokio::test]
async fn a_project_sees_and_removes_only_its_own() {
    let state = state_with_projects().await;
    let in_a = state
        .record_episode(&episode(A, 1, "shared walrus error"))
        .await
        .unwrap();
    let in_b = state
        .record_episode(&episode(B, 2, "shared walrus error"))
        .await
        .unwrap();

    let ids = |episodes: Vec<Episode>| episodes.iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids(found(&state, A, "walrus").await), [in_a]);
    assert_eq!(
        ids(state.list_episodes(A, None, 100).await.unwrap()),
        [in_a]
    );
    assert!(!state.remove_episode(A, in_b).await.unwrap());
    assert_eq!(ids(found(&state, B, "walrus").await), [in_b]);
    assert!(state.remove_episode(B, in_b).await.unwrap());
    assert!(found(&state, B, "walrus").await.is_empty());
    assert!(indexed(&state, "walrus").await == [in_a]);
}

#[tokio::test]
async fn pages_go_newest_first() {
    let state = state_with_projects().await;
    let mut ids = Vec::new();
    for n in 1..=5 {
        ids.push(
            state
                .record_episode(&episode(A, n, "page entry"))
                .await
                .unwrap(),
        );
    }
    let page = |after, limit| state.list_episodes(A, after, limit);
    let first: Vec<i64> = page(None, 2).await.unwrap().iter().map(|e| e.id).collect();
    assert_eq!(first, [ids[4], ids[3]]);
    let second: Vec<i64> = page(Some(ids[3]), 2)
        .await
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(second, [ids[2], ids[1]]);
    assert_eq!(page(None, u32::MAX).await.unwrap().len(), 5);
}

#[tokio::test]
async fn a_deleted_chat_leaves_its_episodes_unlinked() {
    let state = state_with_projects().await;
    state.create_thread("thread", "test").await.unwrap();
    state
        .add_user_message("thread", "uid", "fix the build")
        .await
        .unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    let mut new = episode(A, 1, "linked episode");
    new.thread_id = Some("thread".into());
    new.turn_id = Some(turn.turn_id.clone());
    let id = state.record_episode(&new).await.unwrap();
    assert_eq!(
        state
            .set_turn_lesson(&turn.turn_id, "the flag was missing")
            .await
            .unwrap(),
        1
    );

    state.delete_thread("thread").await.unwrap();
    let kept = state.list_episodes(A, None, 10).await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, id);
    assert_eq!(
        (kept[0].thread_id.as_deref(), kept[0].turn_id.as_deref()),
        (None, None)
    );
    assert_eq!(kept[0].lesson, "the flag was missing");
}

#[tokio::test]
async fn a_removed_project_takes_its_episodes_and_their_index() {
    let state = state_with_projects().await;
    state
        .record_episode(&episode(A, 1, "doomed narwhal"))
        .await
        .unwrap();
    state
        .record_episode(&episode(B, 1, "surviving narwhal"))
        .await
        .unwrap();
    state.remove_project("a").await.unwrap();
    assert!(indexed(&state, "doomed").await.is_empty());
    assert_eq!(indexed(&state, "narwhal").await.len(), 1);
    assert!(!state.episode_counts().await.unwrap().contains_key(A));
    index_is_consistent(&state).await;
    // An unsaved folder has no episodes.
    assert!(
        state
            .record_episode(&episode("/work/c", 1, "no project"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn text_past_its_cap_is_an_error() {
    let state = state_with_projects().await;
    let long = episode(A, 1, &"x".repeat(401));
    assert!(state.record_episode(&long).await.is_err());
    let mut bad_signature = episode(A, 1, "short");
    bad_signature.signature = "abc".into();
    assert!(state.record_episode(&bad_signature).await.is_err());
    state.record_episode(&episode(A, 2, "short")).await.unwrap();
    // A lesson of 201 characters.
    let mut turnless = episode(A, 3, "short");
    turnless.lesson = "y".repeat(201);
    assert!(state.record_episode(&turnless).await.is_err());
    assert_eq!(state.episode_counts().await.unwrap()[A], 1);
}

#[tokio::test]
async fn hostile_queries_match_only_their_words() {
    let state = state_with_projects().await;
    let texts = [
        "alpha failure in the parser",
        "beta crash: problem with quotes \" here",
        "ошибка компоновщика при сборке",
    ];
    for (n, text) in (1..).zip(texts) {
        state.record_episode(&episode(A, n, text)).await.unwrap();
    }
    let queries = [
        "\"".to_owned(),
        "\"\"\"".to_owned(),
        "*".to_owned(),
        "alp*".to_owned(),
        "NEAR(".to_owned(),
        "NEAR(alpha beta, 2)".to_owned(),
        "AND OR NOT".to_owned(),
        "alpha AND NOT beta".to_owned(),
        "problem: zzz".to_owned(),
        "problem:alpha".to_owned(),
        "{problem failed}: beta".to_owned(),
        "(alpha) OR (beta".to_owned(),
        "-alpha".to_owned(),
        "^beta".to_owned(),
        "a\0lpha \0".to_owned(),
        "quotes \" here".to_owned(),
        "ошибка".to_owned(),
        "a b cd ef".to_owned(),
        "x".repeat(10_000),
        "alpha ".repeat(2_000),
        format!("{} alpha", "q ".repeat(150)),
    ];
    for query in &queries {
        let words: Vec<String> = query
            .chars()
            .take(QUERY_CHARS)
            .collect::<String>()
            .split(|c: char| c.is_whitespace() || c.is_control())
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
            .filter(|w| w.chars().count() >= MIN_WORD_CHARS)
            .map(str::to_lowercase)
            .collect();
        match state.recall_episodes(A, query).await {
            Ok(Recall::NoUsableWord) => assert!(words.is_empty(), "{query:?}"),
            Ok(Recall::Found(found)) => {
                for episode in found {
                    let text = episode.problem.to_lowercase();
                    assert!(
                        words.iter().any(|w| text.contains(w.as_str())),
                        "{query:?} found {text:?}"
                    );
                }
            }
            Err(error) => panic!("{query:?}: {error:#}"),
        }
    }
    assert!(found(&state, A, "AND OR NOT").await.is_empty());
    assert_eq!(found(&state, A, "problem: zzz").await.len(), 1);
    assert_eq!(found(&state, A, "ошибка").await.len(), 1);
    assert_eq!(found(&state, A, "\"alpha").await.len(), 1);
    // Only the first 200 characters are read.
    assert_eq!(
        state
            .recall_episodes(A, &format!("{} alpha", "q ".repeat(150)))
            .await
            .unwrap(),
        Recall::NoUsableWord
    );
    assert_eq!(
        state.recall_episodes(A, "a bc ** -").await.unwrap(),
        Recall::NoUsableWord
    );
}

#[tokio::test]
async fn a_match_in_the_problem_ranks_first() {
    let state = state_with_projects().await;
    let mut in_failed = episode(A, 1, "unrelated error text");
    in_failed.failed = "tried the pelican step".into();
    let in_failed = state.record_episode(&in_failed).await.unwrap();
    let mut in_problem = episode(A, 2, "the pelican step broke");
    in_problem.failed = "tried unrelated things".into();
    let in_problem = state.record_episode(&in_problem).await.unwrap();
    let ids: Vec<i64> = found(&state, A, "pelican")
        .await
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, [in_problem, in_failed]);
}
