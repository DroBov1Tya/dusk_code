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
