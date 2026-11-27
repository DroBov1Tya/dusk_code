//! Each chat keeps its own model: choosing one in a chat never moves another chat, its
//! agents or the effort it thinks at. Only chats started afterwards take the new one.

use super::*;
use crate::{config::Effort, init::paths::Paths, runtime::Runtime};

fn choice(provider_name: &str, provider: &Arc<FakeProvider>) -> Choice {
    Choice {
        provider_name: provider_name.into(),
        provider: Arc::clone(provider) as Arc<dyn ModelProvider>,
        model: model(None),
    }
}

fn answers() -> Vec<Vec<Step>> {
    (0..8).map(|_| vec![Step::Text("ok"), Step::Done]).collect()
}

/// Agent steps the provider was asked for.
fn steps(provider: &FakeProvider) -> usize {
    provider.efforts.lock().unwrap().len()
}

#[tokio::test]
async fn choosing_a_model_in_one_chat_leaves_another_and_its_agents_on_theirs() {
    let root = temp_dir("chat_own_model");
    let cwd = root.join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let (qwen, deepseek) = (provider(answers()), provider(answers()));
    runtime.launcher.model.set(choice("qwen", &qwen));
    let first = runtime.new_thread("test", &cwd).await.unwrap();
    let second = runtime.new_thread("test", &cwd).await.unwrap();
    let (first_id, second_id) = (first.thread_id.as_str(), second.thread_id.as_str());

    // DeepSeek is chosen in the second chat only.
    runtime
        .set_model(Some(second_id), choice("deepseek", &deepseek))
        .await
        .unwrap();
    runtime
        .set_effort(Some(second_id), Some(Effort::High))
        .await
        .unwrap();
    let model_of = async |id| runtime.model_of(Some(id)).await.unwrap();
    assert_eq!(model_of(first_id).await.as_deref(), Some("qwen/fake"));
    assert_eq!(model_of(second_id).await.as_deref(), Some("deepseek/fake"));
    let state = runtime.state();
    assert_eq!(
        state.chat_model(first_id).await.unwrap().as_deref(),
        Some("qwen/fake")
    );
    assert_eq!(runtime.effort(Some(first_id)).await.unwrap(), None);
    assert_eq!(
        runtime.effort(Some(second_id)).await.unwrap(),
        Some(Effort::High)
    );

    // An agent of the first chat starts after the switch and still answers on Qwen.
    state
        .create_agent_thread("child", first_id, "agent", "child")
        .await
        .unwrap();
    let workspace = runtime.launcher.paths.thread_workspace("child").unwrap();
    std::fs::create_dir_all(workspace).unwrap();
    runtime
        .launcher
        .deliver("child", first_id, "task", &cwd)
        .await
        .unwrap();
    let asked = async {
        while steps(&qwen) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(20), asked)
        .await
        .expect("the agent asks its chat's model");
    assert_eq!(
        steps(&deepseek),
        0,
        "nothing of the first chat asked DeepSeek"
    );
    assert_eq!(
        qwen.efforts.lock().unwrap()[0],
        None,
        "the agent thinks from its own chat's effort"
    );

    // A chat started now takes the last chosen, and the first keeps Qwen all along.
    let third = runtime.new_thread("test", &cwd).await.unwrap();
    assert_eq!(
        model_of(&third.thread_id).await.as_deref(),
        Some("deepseek/fake")
    );
    assert_eq!(
        runtime.effort(Some(&third.thread_id)).await.unwrap(),
        Some(Effort::High)
    );
    assert_eq!(model_of(first_id).await.as_deref(), Some("qwen/fake"));
}

/// A provider whose first `held` agent steps wait at a gate, then read a file, so the
/// threads that asked it are mid-turn until the gate opens; later steps answer.
struct Gated {
    gate: tokio::sync::Semaphore,
    held: std::sync::atomic::AtomicUsize,
    to_hold: usize,
    efforts: Mutex<Vec<Option<Effort>>>,
}

impl Gated {
    fn held(&self) -> usize {
        self.held.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl ModelProvider for Gated {
    fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn stream_chat<'a>(&'a self, request: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
        let agent_step = !request.tools.is_empty();
        let held = agent_step && {
            let mut efforts = self.efforts.lock().unwrap();
            efforts.push(request.effort);
            efforts.len() <= self.to_hold
        };
        Box::pin(async move {
            let steps = if held {
                self.held.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.gate.acquire().await?.forget();
                let read = json!({ "path": "notes.txt" });
                vec![Step::Named("read_file", "read_1".into(), read), Step::Done]
            } else {
                vec![Step::Text("ok"), Step::Done]
            };
            let stream = FakeStream {
                steps: steps.into(),
                pending: VecDeque::new(),
            };
            Ok(Box::new(stream) as Box<dyn ChatStream>)
        })
    }
}

#[tokio::test]
async fn a_chat_mid_turn_and_its_agent_at_work_keep_their_model_when_another_switches() {
    let root = temp_dir("chat_own_model_mid_turn");
    let cwd = root.join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(cwd.join("notes.txt"), "notes").unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let qwen = Arc::new(Gated {
        gate: tokio::sync::Semaphore::new(0),
        held: 0.into(),
        to_hold: 2,
        efforts: Mutex::new(Vec::new()),
    });
    let deepseek = provider(answers());
    runtime.launcher.model.set(Choice {
        provider_name: "qwen".into(),
        provider: Arc::clone(&qwen) as Arc<dyn ModelProvider>,
        model: model(None),
    });
    let first = runtime.new_thread("test", &cwd).await.unwrap();
    let second = runtime.new_thread("test", &cwd).await.unwrap();
    let (first_id, second_id) = (first.thread_id.as_str(), second.thread_id.as_str());

    // The first chat and an agent of it are both mid-turn on Qwen.
    let state = runtime.state();
    state
        .create_agent_thread("child", first_id, "agent", "child")
        .await
        .unwrap();
    let workspace = runtime.launcher.paths.thread_workspace("child").unwrap();
    std::fs::create_dir_all(workspace).unwrap();
    runtime
        .launcher
        .deliver("child", first_id, "task", &cwd)
        .await
        .unwrap();
    first.input.send("read the notes").await.unwrap();
    let both_held = async {
        while qwen.held() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(20), both_held)
        .await
        .expect("the chat and its agent are mid-turn");

    // DeepSeek is chosen in the second chat while they work.
    runtime
        .set_model(Some(second_id), choice("deepseek", &deepseek))
        .await
        .unwrap();
    runtime
        .set_effort(Some(second_id), Some(Effort::High))
        .await
        .unwrap();
    qwen.gate.add_permits(2);

    // Each takes its next step on Qwen, at its own chat's effort.
    let next_steps = async {
        while qwen.efforts.lock().unwrap().len() < 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(20), next_steps)
        .await
        .expect("the chat and its agent take their next step on Qwen");
    assert_eq!(
        steps(&deepseek),
        0,
        "nothing of the first chat asked DeepSeek"
    );
    assert!(
        qwen.efforts.lock().unwrap().iter().all(Option::is_none),
        "no step of the first chat thinks at the second chat's effort"
    );
    let model_of = runtime.model_of(Some(first_id)).await.unwrap();
    assert_eq!(model_of.as_deref(), Some("qwen/fake"));
}
