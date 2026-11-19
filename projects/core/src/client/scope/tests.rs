//! The policy against one request of every kind.

use std::collections::BTreeSet;

use dusk_protocol::{
    client::{CatalogRequest, ProjectRequest, Request, SettingsRequest, ThreadRequest},
    plugin::StoreRequest,
    tools::Decision,
};

use super::{
    Area, Scope, Subject, classify,
    names::{NAMES, name},
    samples::{THREAD, samples},
};

/// What the web bridge refused before the scope, as its `refused` had it.
fn bridge_refused(request: &Request) -> bool {
    matches!(
        request,
        Request::Shutdown
            | Request::Catalog(
                CatalogRequest::McpServerText { .. }
                    | CatalogRequest::SaveMcpServer { old: Some(_), .. }
            )
            | Request::Terminal(_)
            | Request::Plugin(_)
            | Request::Store(
                StoreRequest::Install { .. }
                    | StoreRequest::Review { .. }
                    | StoreRequest::Remove { .. }
                    | StoreRequest::UpdateAll
                    | StoreRequest::Probe { .. }
                    | StoreRequest::AddSource { .. }
                    | StoreRequest::RemoveSource { .. }
            )
            | Request::Settings(SettingsRequest::PluginEnabled { enabled: true, .. })
            | Request::Project(ProjectRequest::WriteFile { .. })
    )
}

fn permitted(scope: &Scope) -> BTreeSet<&'static str> {
    samples()
        .iter()
        .filter(|request| scope.permits(request))
        .map(name)
        .collect()
}

#[test]
fn every_kind_of_request_has_one_sample() {
    let sampled: Vec<&str> = samples().iter().map(name).collect();
    let unique: BTreeSet<&str> = sampled.iter().copied().collect();
    assert_eq!(sampled.len(), unique.len(), "a kind is sampled twice");
    assert_eq!(unique, NAMES.iter().copied().collect::<BTreeSet<_>>());
    assert_eq!(NAMES.len(), unique.len(), "a name is used twice");
}

#[test]
fn the_web_scope_refuses_exactly_what_the_bridge_refused() {
    let web = Scope::web();
    for request in samples() {
        assert_eq!(
            !web.permits(&request),
            bridge_refused(&request),
            "{}",
            name(&request)
        );
    }
}

#[test]
fn the_full_scope_takes_every_request() {
    let full = Scope::full();
    assert!(samples().iter().all(|request| full.permits(request)));
    assert!(full.covers("any") && full.reads_status());
    assert!(Scope::web().covers("any") && Scope::web().reads_status());
}

/// Every area there is; the match keeps the list whole.
fn every_area() -> &'static [Area] {
    const ALL: &[Area] = &[
        Area::ThreadRead,
        Area::ThreadWrite,
        Area::ThreadCreate,
        Area::ThreadDelete,
        Area::ThreadCommand,
        Area::ThreadApprove,
        Area::ThreadApprovals,
        Area::Settings,
        Area::PluginEnable,
        Area::Catalog,
        Area::McpConfig,
        Area::Project,
        Area::FileWrite,
        Area::Command,
        Area::System,
        Area::Voice,
        Area::Terminal,
        Area::PluginParts,
        Area::Store,
        Area::StoreInstall,
        Area::Update,
        Area::Shutdown,
    ];
    for area in ALL {
        match area {
            Area::ThreadRead
            | Area::ThreadWrite
            | Area::ThreadCreate
            | Area::ThreadDelete
            | Area::ThreadCommand
            | Area::ThreadApprove
            | Area::ThreadApprovals
            | Area::Settings
            | Area::PluginEnable
            | Area::Catalog
            | Area::McpConfig
            | Area::Project
            | Area::FileWrite
            | Area::Command
            | Area::System
            | Area::Voice
            | Area::Terminal
            | Area::PluginParts
            | Area::Store
            | Area::StoreInstall
            | Area::Update
            | Area::Shutdown => {}
        }
    }
    ALL
}

#[test]
fn a_scope_without_areas_takes_nothing() {
    assert!(permitted(&Scope::threads([THREAD.to_owned()], &[])).is_empty());
    assert!(permitted(&Scope::threads([], every_area())).len() < NAMES.len());
}

/// Files, the shell, settings, secrets, plugins, MCP, projects, updates and shutdown are
/// about no thread, so no list of areas opens them to a scope bound to threads.
#[test]
fn a_scope_bound_to_threads_reaches_nothing_but_threads() {
    let scope = Scope::threads([THREAD.to_owned()], every_area());
    assert!(!scope.reads_status());
    assert!(scope.covers(THREAD) && !scope.covers("thread-b"));
    for request in samples() {
        let access = classify(&request);
        let about_threads = access.subject != Subject::None;
        assert_eq!(scope.permits(&request), about_threads, "{}", name(&request));
        let kept_out = matches!(
            request,
            Request::Settings(_)
                | Request::Catalog(_)
                | Request::Project(_)
                | Request::Terminal(_)
                | Request::Plugin(_)
                | Request::Store(_)
                | Request::Voice(_)
                | Request::SystemReport
                | Request::InstallUpdate { .. }
                | Request::Shutdown
        );
        assert!(!(kept_out && about_threads), "{}", name(&request));
    }
}

/// The whole of what a scope for reading and driving shared chats may send. A new request
/// that lands in these areas fails here until it is added on purpose.
#[test]
fn a_thread_bound_scope_without_thread_approve_cannot_answer() {
    let approve = |thread_id: &str| {
        Request::Thread(ThreadRequest::Approve {
            thread_id: thread_id.into(),
            id: 1,
            decision: Decision::Once,
        })
    };
    let scope = Scope::threads(["a".to_owned()], &[Area::ThreadRead, Area::ThreadWrite]);
    assert!(!scope.permits(&approve("a")));
    let scope = Scope::threads(["a".to_owned()], &[Area::ThreadApprove]);
    assert!(scope.permits(&approve("a")));
    assert!(scope.covers("a") && !scope.covers("b"));
}

#[test]
fn reading_and_writing_shared_threads_is_this_list_and_no_more() {
    let scope = Scope::threads([THREAD.to_owned()], &[Area::ThreadRead, Area::ThreadWrite]);
    let expected = [
        "command.jobs.thread",
        "command.resume",
        "command.sources.thread",
        "command.undo.points",
        "thread.background",
        "thread.cancel",
        "thread.close",
        "thread.get_approvals",
        "thread.list",
        "thread.local_unsent",
        "thread.messages",
        "thread.pin",
        "thread.pins",
        "thread.rename",
        "thread.resend_local_unsent",
        "thread.resume",
        "thread.search",
        "thread.send",
        "thread.skill_changes",
        "thread.stop_agent",
        "thread.tasks",
        "work.card",
        "work.changed_files",
        "work.confirm_task",
        "work.events",
        "work.file_changes",
        "work.job_output",
        "work.jobs",
        "work.overview",
        "work.pin_source",
        "work.plan",
        "work.plan_tasks",
        "work.remove_source",
        "work.save_plan",
        "work.source_picture",
        "work.stop_job",
        "work.stop_team",
        "work.team",
        "work.upload_picture",
    ];
    assert_eq!(permitted(&scope), BTreeSet::from(expected));

    let reading = permitted(&Scope::threads([THREAD.to_owned()], &[Area::ThreadRead]));
    let changes = [
        "thread.background",
        "thread.cancel",
        "thread.close",
        "thread.pin",
        "thread.rename",
        "thread.resend_local_unsent",
        "thread.resume",
        "thread.send",
        "thread.stop_agent",
        "work.confirm_task",
        "work.pin_source",
        "work.remove_source",
        "work.save_plan",
        "work.stop_job",
        "work.stop_team",
        "work.upload_picture",
    ];
    let mut read_only = BTreeSet::from(expected);
    read_only.retain(|name| !changes.contains(name));
    assert_eq!(reading, read_only);
}

/// The whole of what a browser may send, by name: a new request that lands in an area of
/// the web scope fails here until someone decides a browser may send it.
#[test]
fn a_browser_is_refused_this_list_and_no_more() {
    let allowed = permitted(&Scope::web());
    let refused: BTreeSet<&str> = NAMES
        .iter()
        .copied()
        .filter(|name| !allowed.contains(name))
        .collect();
    let expected = [
        "catalog.mcp_server_text",
        "catalog.save_mcp_server.saved",
        "plugin.close",
        "plugin.event",
        "plugin.open",
        "plugin.redetect",
        "project.write_file",
        "settings.plugin_enabled.on",
        "shutdown",
        "store.add_source",
        "store.install",
        "store.probe",
        "store.remove",
        "store.remove_source",
        "store.review",
        "store.update_all",
        "terminal.close",
        "terminal.input",
        "terminal.open",
        "terminal.resize",
    ];
    assert_eq!(refused, BTreeSet::from(expected));
    assert_eq!(allowed.len(), NAMES.len() - expected.len());
}

#[test]
fn skill_diff_and_undo_need_catalog_and_no_thread_bound_scope() {
    use dusk_protocol::client::ThreadRequest;
    let diff = Request::Catalog(CatalogRequest::SkillDiff { id: 1 });
    let undo = Request::Catalog(CatalogRequest::UndoSkillChange { id: 1 });
    for request in [&diff, &undo] {
        assert_eq!(classify(request).area, Area::Catalog);
        assert!(!Scope::threads([THREAD.to_owned()], every_area()).permits(request));
        assert!(!Scope::threads([], &[Area::ThreadRead, Area::ThreadWrite]).permits(request));
    }
    let listed = Request::Thread(ThreadRequest::SkillChanges {
        thread_id: THREAD.to_owned(),
    });
    assert!(Scope::threads([THREAD.to_owned()], &[Area::ThreadRead]).permits(&listed));
    let scope = Scope::threads([THREAD.to_owned()], &[Area::ThreadRead]);
    assert!(scope.covers(THREAD) && !scope.covers("thread-b"));
}
