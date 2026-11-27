//! The area of each request and the thread it is about. Every variant is named: a new
//! request does not compile until it is given an area here.

use dusk_protocol::{
    client::{
        CatalogRequest as Catalog, Command, PluginRequest as Plugin, ProjectRequest as Project,
        Request, SettingsRequest as Settings, TerminalRequest as Terminal, ThreadRequest as Thread,
        VoiceRequest as Voice, WorkRequest as Work,
    },
    plugin::StoreRequest as Store,
};

use super::Area;

/// Which thread a request is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject<'a> {
    /// This one, by its id.
    Thread(&'a str),
    /// Any of them: the answer lists threads, and is cut down to the scope's.
    Threads,
    /// None, or one that does not exist yet.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access<'a> {
    pub area: Area,
    pub subject: Subject<'a>,
}

fn of(area: Area, thread_id: &str) -> Access<'_> {
    Access {
        area,
        subject: Subject::Thread(thread_id),
    }
}

fn all(area: Area) -> Access<'static> {
    Access {
        area,
        subject: Subject::Threads,
    }
}

fn apart(area: Area) -> Access<'static> {
    Access {
        area,
        subject: Subject::None,
    }
}

pub fn classify(request: &Request) -> Access<'_> {
    match request {
        Request::Thread(request) => thread(request),
        Request::Work(request) => work(request),
        Request::Command(command) => command_access(command),
        Request::Settings(request) => apart(settings(request)),
        Request::Catalog(request) => apart(catalog(request)),
        Request::Project(request) => apart(project(request)),
        Request::Voice(
            Voice::Transcribe { .. }
            | Voice::Speak { .. }
            | Voice::Models
            | Voice::Install { .. }
            | Voice::Cancel { .. }
            | Voice::Remove { .. },
        ) => apart(Area::Voice),
        Request::Terminal(
            Terminal::Open { .. }
            | Terminal::Input { .. }
            | Terminal::Resize { .. }
            | Terminal::Close { .. },
        ) => apart(Area::Terminal),
        Request::Plugin(
            Plugin::Open { .. }
            | Plugin::Redetect { .. }
            | Plugin::Close { .. }
            | Plugin::Event { .. },
        ) => apart(Area::PluginParts),
        Request::Store(
            Store::List { .. }
            | Store::Cover { .. }
            | Store::Screenshot { .. }
            | Store::Contents { .. }
            | Store::File { .. }
            | Store::Sources,
        ) => apart(Area::Store),
        Request::Store(
            Store::Install { .. }
            | Store::Review { .. }
            | Store::Remove { .. }
            | Store::UpdateAll
            | Store::Probe { .. }
            | Store::AddSource { .. }
            | Store::RemoveSource { .. },
        ) => apart(Area::StoreInstall),
        Request::SystemReport => apart(Area::System),
        Request::InstallUpdate { .. } => apart(Area::Update),
        Request::Shutdown => apart(Area::Shutdown),
        Request::Clients | Request::Disconnect { .. } => apart(Area::Clients),
    }
}

fn thread(request: &Thread) -> Access<'_> {
    match request {
        Thread::New { .. } | Thread::NewTemporary { .. } | Thread::Fork { .. } => {
            apart(Area::ThreadCreate)
        }
        // A new thread, but with what one thread said: only from a thread in reach.
        Thread::ForkAt { thread_id, .. } => of(Area::ThreadCreate, thread_id),
        Thread::List | Thread::Search { .. } => all(Area::ThreadRead),
        Thread::Messages { thread_id }
        | Thread::Tasks { thread_id }
        | Thread::SkillChanges { thread_id }
        | Thread::LocalUnsent { thread_id }
        | Thread::GetApprovals { thread_id }
        | Thread::Pins { thread_id }
        // Showing a chat is reading it, and what it changes is only whether it is read.
        | Thread::Seen { thread_id } => of(Area::ThreadRead, thread_id),
        Thread::Resume { thread_id, .. }
        | Thread::ResendLocalUnsent { thread_id, .. }
        | Thread::Send { thread_id, .. }
        | Thread::Cancel { thread_id }
        | Thread::Background { thread_id, .. }
        | Thread::Close { thread_id }
        | Thread::StopAgent { thread_id }
        | Thread::Rename { thread_id, .. }
        | Thread::Pin { thread_id, .. } => of(Area::ThreadWrite, thread_id),
        Thread::Approve { thread_id, .. } => of(Area::ThreadApprove, thread_id),
        Thread::RunCommand { thread_id, .. } | Thread::StopRun { thread_id, .. } => {
            of(Area::ThreadCommand, thread_id)
        }
        Thread::Delete { thread_id } => of(Area::ThreadDelete, thread_id),
        Thread::SetApprovals { thread_id, .. } => of(Area::ThreadApprovals, thread_id),
    }
}

fn work(request: &Work) -> Access<'_> {
    match request {
        Work::Jobs { thread_id }
        | Work::JobOutput { thread_id, .. }
        | Work::SourcePicture { thread_id, .. }
        | Work::Card { thread_id, .. }
        | Work::Plan { thread_id }
        | Work::ChangedFiles { thread_id }
        | Work::FileChanges { thread_id }
        | Work::PlanTasks { thread_id, .. }
        | Work::Team { thread_id }
        | Work::Overview { thread_id }
        | Work::Events { thread_id, .. } => of(Area::ThreadRead, thread_id),
        // The diff is of the whole project folder, whoever changed it, so it is about
        // the project and no one thread: a scope bound to threads does not get it.
        Work::Diff { .. } => apart(Area::Project),
        Work::StopJob { thread_id, .. }
        | Work::PinSource { thread_id, .. }
        | Work::RemoveSource { thread_id, .. }
        | Work::UploadPicture { thread_id, .. }
        | Work::SavePlan { thread_id, .. }
        | Work::ConfirmTask { thread_id, .. }
        | Work::RetryTask { thread_id, .. }
        | Work::StopTeam { thread_id } => of(Area::ThreadWrite, thread_id),
        // How many members work at once, a setting of the team, so it sits with the approval
        // mode.
        Work::SetTeamAtOnce { thread_id, .. } => of(Area::ThreadApprovals, thread_id),
    }
}

fn command_access(command: &Command) -> Access<'_> {
    match command {
        Command::Resume { .. } => all(Area::ThreadRead),
        Command::Jobs {
            thread_id: Some(thread_id),
        }
        | Command::Undo {
            thread_id: Some(thread_id),
            sequence: None,
        }
        | Command::Sources {
            thread_id: Some(thread_id),
            add: None,
        } => of(Area::ThreadRead, thread_id),
        // It copies a file of this machine, by a path the client names.
        Command::Sources {
            thread_id: Some(thread_id),
            add: Some(_),
        } => of(Area::Command, thread_id),
        // The report names the skills and MCP tools of this machine and counts every
        // thread's use, with a thread or without: it is about no one thread.
        Command::Context { .. }
        | Command::Jobs { thread_id: None }
        | Command::Undo { .. }
        | Command::Sources {
            thread_id: None, ..
        }
        | Command::Migrate { .. }
        | Command::Compact { .. }
        | Command::New
        | Command::Temporary
        | Command::Model { .. }
        | Command::Provider { .. }
        | Command::Settings
        | Command::Fork { .. }
        | Command::Update { .. }
        | Command::Effort { .. }
        | Command::Goal { .. } => apart(Area::Command),
    }
}

fn settings(request: &Settings) -> Area {
    match request {
        Settings::PluginEnabled { enabled: true, .. } => Area::PluginEnable,
        Settings::PluginEnabled { enabled: false, .. }
        | Settings::ContextPolicy(_)
        | Settings::MaxAgentsAtWork(_)
        | Settings::IdleScreenMinutes(_)
        | Settings::ModelTimeout(_)
        | Settings::MaxAttempts(_)
        | Settings::MemoryChars(_)
        | Settings::WorkHistoryChars(_)
        | Settings::Switch { .. }
        | Settings::BranchPrefix(_)
        | Settings::Arbiter(_)
        | Settings::ModelTier { .. }
        | Settings::NextMode
        | Settings::Mode(_)
        | Settings::Connect { .. }
        | Settings::SignInChatGpt
        | Settings::CheckProvider(_)
        | Settings::ReconnectProvider { .. }
        | Settings::RemoveProvider { .. }
        | Settings::ProviderEnabled { .. }
        | Settings::RemoveAllProviders
        | Settings::SetSecret { .. }
        | Settings::DeleteSecret { .. }
        | Settings::Voice(_)
        | Settings::SetContractPlugin { .. }
        | Settings::PluginSecret { .. } => Area::Settings,
    }
}

fn catalog(request: &Catalog) -> Area {
    match request {
        Catalog::McpServerText { .. } | Catalog::SaveMcpServer { old: Some(_), .. } => {
            Area::McpConfig
        }
        Catalog::SaveMcpServer { old: None, .. }
        | Catalog::Skill { .. }
        | Catalog::SaveSkill { .. }
        | Catalog::SkillPacks
        | Catalog::SkillDiff { .. }
        | Catalog::UndoSkillChange { .. }
        | Catalog::Agent { .. }
        | Catalog::SaveAgent { .. }
        | Catalog::McpServers
        | Catalog::RestartMcp { .. }
        | Catalog::SignInMcp { .. }
        | Catalog::Snippets
        | Catalog::SaveSnippet { .. }
        | Catalog::Profile
        | Catalog::SaveProfile(_)
        | Catalog::Secrets
        | Catalog::Voice => Area::Catalog,
    }
}

fn project(request: &Project) -> Area {
    match request {
        Project::WriteFile { .. } => Area::FileWrite,
        Project::List
        | Project::Save { .. }
        | Project::Remove { .. }
        | Project::Git { .. }
        | Project::Diff { .. }
        | Project::SwitchBranch { .. }
        // As a branch switch: a client that may change projects may change their
        // repositories. A scope bound to threads takes none of them; push and discard
        // are asked of the user by the client before they are sent.
        | Project::Repository { .. }
        | Project::RepositoryStatus { .. }
        | Project::PullStatus { .. }
        | Project::CreatePull { .. }
        | Project::PullSummaries
        | Project::Files { .. }
        | Project::Outputs
        | Project::Env { .. }
        | Project::SetEnv { .. }
        | Project::RemoveEnv { .. }
        | Project::ReadOutput { .. }
        | Project::ReadFile { .. }
        | Project::DeleteOutput { .. }
        | Project::Schedules
        | Project::SaveSchedule { .. }
        | Project::DeleteSchedule { .. }
        | Project::RunSchedule { .. } => Area::Project,
    }
}
