//! One request of every kind `names` knows, for the tests of the policy.

use dusk_protocol::{
    client::{
        CatalogRequest as C, Command as Cmd, PluginRequest as Pl, ProjectRequest as P,
        ProviderTarget, Request as R, Secret, SettingsRequest as S, Switch, TerminalRequest as Te,
        ThreadRequest as T, VoiceRequest as V, WorkRequest as W,
    },
    config::{ContextPolicy, Profile, ProviderKind, VoiceSettings},
    plugin::{StoreRequest as St, UiEvent},
    project::{Repeat, ScheduleDraft},
    thread::TeamBudget,
    tools::Decision,
};

/// The thread every sample that names one is about.
pub const THREAD: &str = "thread-a";

fn id() -> String {
    THREAD.to_owned()
}

fn word() -> String {
    "x".to_owned()
}

fn secret() -> Secret {
    Secret("s".to_owned())
}

/// One request of every name in `NAMES`.
pub fn samples() -> Vec<R> {
    let mut all = Vec::new();
    all.extend(threads().into_iter().map(R::Thread));
    all.extend(work().into_iter().map(R::Work));
    all.extend(settings().into_iter().map(R::Settings));
    all.extend(catalog().into_iter().map(R::Catalog));
    all.extend(projects().into_iter().map(R::Project));
    all.extend(commands().into_iter().map(R::Command));
    all.extend(rest());
    all
}

fn threads() -> Vec<T> {
    vec![
        T::New { cwd: "/".into() },
        T::NewTemporary { cwd: "/".into() },
        T::Resume {
            thread_id: id(),
            cwd: "/".into(),
        },
        T::Fork {
            summary: word(),
            cwd: "/".into(),
        },
        T::List,
        T::Search { text: word() },
        T::Messages { thread_id: id() },
        T::Tasks { thread_id: id() },
        T::LocalUnsent { thread_id: id() },
        T::ResendLocalUnsent {
            thread_id: id(),
            id: word(),
        },
        T::Send {
            thread_id: id(),
            text: word(),
            now: false,
        },
        T::Cancel { thread_id: id() },
        T::Background {
            thread_id: id(),
            call_id: word(),
        },
        T::RunCommand {
            thread_id: id(),
            command: word(),
        },
        T::Close { thread_id: id() },
        T::StopAgent { thread_id: id() },
        T::Rename {
            thread_id: id(),
            title: word(),
        },
        T::Delete { thread_id: id() },
        T::Approve {
            id: 1,
            decision: Decision::Deny,
        },
        T::SetApprovals {
            thread_id: id(),
            policy: None,
        },
        T::GetApprovals { thread_id: id() },
        T::Pin {
            thread_id: id(),
            message_id: word(),
            pinned: true,
        },
        T::Pins { thread_id: id() },
    ]
}

fn work() -> Vec<W> {
    vec![
        W::Jobs { thread_id: id() },
        W::JobOutput {
            thread_id: id(),
            job_id: word(),
        },
        W::StopJob {
            thread_id: id(),
            job_id: word(),
        },
        W::PinSource {
            thread_id: id(),
            id: word(),
            pinned: true,
        },
        W::RemoveSource {
            thread_id: id(),
            id: word(),
        },
        W::UploadPicture {
            thread_id: id(),
            name: word(),
            bytes: Vec::new(),
        },
        W::SourcePicture {
            thread_id: id(),
            name: word(),
        },
        W::Plan { thread_id: id() },
        W::SavePlan {
            thread_id: id(),
            text: word(),
        },
        W::Diff {
            thread_id: id(),
            path: None,
        },
        W::ChangedFiles { thread_id: id() },
        W::FileChanges { thread_id: id() },
        W::PlanTasks {
            thread_id: id(),
            done_before: None,
            limit: 1,
        },
        W::Team { thread_id: id() },
        W::Overview { thread_id: id() },
        W::Events {
            thread_id: id(),
            before: None,
            limit: 1,
        },
        W::ConfirmTask {
            thread_id: id(),
            task_id: word(),
        },
        W::StopTeam { thread_id: id() },
        W::SetTeamBudget {
            thread_id: id(),
            budget: TeamBudget::new(1, 1, 1, 1).expect("a budget within the caps"),
        },
    ]
}

fn settings() -> Vec<S> {
    let plugin = |enabled| S::PluginEnabled {
        name: word(),
        enabled,
    };
    vec![
        S::ContextPolicy(ContextPolicy::Auto),
        S::MaxAgentsAtWork(1),
        S::IdleScreenMinutes(1),
        S::MaxGoalRounds(1),
        S::ModelTimeout(1),
        S::MaxAttempts(1),
        S::MemoryChars(1),
        S::WorkHistoryChars(1),
        S::Switch {
            switch: Switch::Memory,
            on: true,
        },
        S::BranchPrefix(word()),
        S::NextMode,
        S::Mode(word()),
        S::Connect {
            kind: ProviderKind::OpenAi,
            name: None,
            base_url: word(),
            api_key: Some(secret()),
        },
        S::SignInChatGpt,
        S::CheckProvider(ProviderTarget::Saved { name: word() }),
        S::ReconnectProvider {
            name: word(),
            api_key: None,
        },
        S::RemoveProvider { name: word() },
        S::ProviderEnabled {
            name: word(),
            enabled: true,
        },
        S::RemoveAllProviders,
        S::SetSecret {
            name: word(),
            value: secret(),
        },
        S::DeleteSecret { name: word() },
        S::Voice(VoiceSettings::default()),
        plugin(true),
        plugin(false),
        S::PluginSecret {
            plugin: word(),
            key: word(),
            value: secret(),
        },
    ]
}

fn catalog() -> Vec<C> {
    let named = |name: fn(String) -> C| name(word());
    let saved = |save: fn(String, String) -> C| save(word(), word());
    vec![
        named(|name| C::Skill { name }),
        saved(|name, text| C::SaveSkill { name, text }),
        C::SkillPacks,
        named(|name| C::Agent { name }),
        saved(|name, text| C::SaveAgent { name, text }),
        C::McpServers,
        named(|name| C::McpServerText { name }),
        C::SaveMcpServer {
            old: Some(word()),
            text: word(),
        },
        C::SaveMcpServer {
            old: None,
            text: word(),
        },
        named(|name| C::RestartMcp { name }),
        named(|name| C::SignInMcp { name }),
        C::Snippets,
        saved(|name, text| C::SaveSnippet { name, text }),
        C::Profile,
        C::SaveProfile(Profile::default()),
        C::Secrets,
        C::Voice,
    ]
}

fn projects() -> Vec<P> {
    vec![
        P::List,
        P::Save {
            id: None,
            name: word(),
            description: word(),
            dir: "/".into(),
        },
        P::Remove { id: word() },
        P::Git { id: word() },
        P::Diff {
            id: word(),
            path: None,
        },
        P::SwitchBranch {
            id: word(),
            branch: word(),
        },
        P::Files {
            id: word(),
            path: word(),
        },
        P::Outputs,
        P::Env { id: word() },
        P::SetEnv {
            id: word(),
            name: word(),
            value: secret(),
        },
        P::RemoveEnv {
            id: word(),
            name: word(),
        },
        P::ReadOutput { path: "/x".into() },
        P::ReadFile { path: "/x".into() },
        P::WriteFile {
            path: "/x".into(),
            text: word(),
            seen: None,
        },
        P::DeleteOutput { path: "/x".into() },
        P::Schedules,
        P::SaveSchedule {
            draft: ScheduleDraft {
                id: None,
                project_id: word(),
                prompt: word(),
                repeat: Repeat::Once { at: 0 },
                enabled: true,
                model: None,
            },
        },
        P::DeleteSchedule { id: word() },
        P::RunSchedule { id: word() },
    ]
}

fn commands() -> Vec<Cmd> {
    let undo = |thread_id, sequence| Cmd::Undo {
        thread_id,
        sequence,
    };
    let sources = |thread_id, add| Cmd::Sources { thread_id, add };
    vec![
        Cmd::Migrate { path: None },
        Cmd::Compact { instructions: None },
        Cmd::Resume { cwd: None },
        Cmd::New,
        Cmd::Temporary,
        Cmd::Model { name: None },
        Cmd::Provider { name: None },
        Cmd::Settings,
        Cmd::Context {
            thread_id: Some(id()),
        },
        Cmd::Context { thread_id: None },
        Cmd::Fork { instructions: None },
        Cmd::Update { channel: None },
        Cmd::Effort { level: None },
        Cmd::Goal { argument: None },
        undo(Some(id()), None),
        undo(Some(id()), Some(1)),
        undo(None, None),
        sources(Some(id()), None),
        sources(Some(id()), Some(word())),
        sources(None, None),
        Cmd::Jobs {
            thread_id: Some(id()),
        },
        Cmd::Jobs { thread_id: None },
    ]
}

fn rest() -> Vec<R> {
    let part = |request: fn(String, String) -> Pl| R::Plugin(request(word(), word()));
    vec![
        R::SystemReport,
        R::Voice(V::Transcribe { audio: Vec::new() }),
        R::Voice(V::Speak { text: word() }),
        R::Voice(V::Models),
        R::Voice(V::Install { id: word() }),
        R::Voice(V::Cancel { id: word() }),
        R::Voice(V::Remove { id: word() }),
        R::Terminal(Te::Open {
            cwd: "/".into(),
            cols: 80,
            rows: 24,
        }),
        R::Terminal(Te::Input {
            id: word(),
            data: Vec::new(),
        }),
        R::Terminal(Te::Resize {
            id: word(),
            cols: 80,
            rows: 24,
        }),
        R::Terminal(Te::Close { id: word() }),
        part(|plugin, part| Pl::Open { plugin, part }),
        part(|plugin, part| Pl::Redetect { plugin, part }),
        part(|plugin, part| Pl::Close { plugin, part }),
        part(|plugin, part| Pl::Event {
            plugin,
            part,
            widget: word(),
            event: UiEvent::Click,
        }),
        R::Store(St::List {
            order: Default::default(),
            category: None,
        }),
        R::Store(St::Install {
            source: word(),
            tier: Default::default(),
            name: word(),
            version: word(),
            agreed: Vec::new(),
        }),
        R::Store(St::Remove { name: word() }),
        R::Store(St::UpdateAll),
        R::Store(St::Cover {
            source: word(),
            name: word(),
        }),
        R::Store(St::Screenshot {
            source: word(),
            name: word(),
            index: 0,
        }),
        R::InstallUpdate { tag: word() },
        R::Shutdown,
    ]
}
