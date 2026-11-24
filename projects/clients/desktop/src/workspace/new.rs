//! The view's first state: its boxes, what it reads from the core at start, and the
//! notes a new version leaves.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use dusk_protocol::client::{Event, Status};
use gpui_kit::{
    component::input::{InputEvent, InputState, TextareaState},
    *,
};
use tokio::sync::{mpsc, watch};

use super::{Center, Right, Workspace};
use crate::{
    core::Core,
    pages::{customize::Shelf, settings::SettingsPage},
};

impl Workspace {
    pub fn new(
        core: Core,
        home: PathBuf,
        events: mpsc::UnboundedReceiver<Event>,
        status: watch::Receiver<Status>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Find a chat by its title or by what was said")
        });
        cx.subscribe_in(&search, window, |ws, _, _: &InputEvent, _, cx| {
            ws.search_text(cx);
            cx.notify();
        })
        .detach();
        let shelf_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        cx.subscribe_in(&shelf_search, window, |_, _, _: &InputEvent, _, cx| {
            cx.notify()
        })
        .detach();
        let font_search = cx.new(|cx| InputState::new(window, cx).placeholder("Find a font"));
        cx.subscribe_in(&font_search, window, |_, _, _: &InputEvent, _, cx| {
            cx.notify()
        })
        .detach();
        let model_input = cx.new(|cx| InputState::new(window, cx).placeholder("provider/model"));
        cx.subscribe_in(&model_input, window, |ws, _, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                ws.use_typed_model(cx);
            }
        })
        .detach();
        let limit_edit = crate::pages::settings::LimitEdit::new(window, cx);
        cx.subscribe_in(
            &limit_edit.input,
            window,
            |ws, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    ws.commit_limit(cx);
                }
            },
        )
        .detach();
        let web_port = cx.new(|cx| InputState::new(window, cx).placeholder("8080"));
        let branch_input = cx.new(|cx| InputState::new(window, cx).placeholder("such as dusk/"));
        cx.subscribe_in(&branch_input, window, |ws, _, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                ws.save_branch_prefix(cx);
            }
        })
        .detach();
        let composer = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder(crate::pages::chat::composer::PLACEHOLDER);
            state.set_submit_on_enter(true, cx);
            state
        });
        cx.subscribe_in(
            &composer,
            window,
            |this, _, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { shift: false, .. } => this.send(window, cx),
                    // An `@` word lists what it may name above the box.
                    InputEvent::Change => cx.notify(),
                    _ => {}
                }
            },
        )
        .detach();
        // A chat shown while the window was in the back is read once it comes forward.
        cx.observe_window_activation(window, |_, _, cx| cx.notify())
            .detach();
        let first = status.borrow().clone();
        let mut workspace = Self {
            core,
            home,
            status: first,
            projects: Vec::new(),
            schedules: None,
            threads: Vec::new(),
            chats: HashMap::new(),
            shown: None,
            project: None,
            collapsed: crate::ui::theme::prefs(cx)
                .collapsed_projects
                .iter()
                .cloned()
                .collect(),
            center: Center::Chat,
            shelf: Shelf::Connectors,
            shelf_search,
            shelf_scroll: ScrollHandle::new(),
            todo_done_open: false,
            folded: Right::folded_from(crate::ui::theme::prefs(cx).right_folded.as_deref()),
            right_order: crate::columns::inspector::order::saved(
                &crate::ui::theme::prefs(cx).right_order,
            ),
            module_drop: None,
            module_moved: None,
            right_screen: None,
            right_moves: 0,
            right_scroll: ScrollHandle::new(),
            modal: None,
            settings_page: SettingsPage::Ai,
            ai_tab: crate::pages::settings::ai::Tab::Llm,
            font_search,
            fonts: crate::ui::theme::font_families(cx),
            model_input,
            branch_input,
            web_port,
            outputs: Vec::new(),
            artifact_kind: None,
            search,
            found: Default::default(),
            jump: None,
            temporary_next: false,
            composer,
            fill_composer: Vec::new(),
            drafts: Default::default(),
            me: crate::pages::settings::about_me::AboutMe::new(window, cx),
            arbiter: crate::pages::settings::arbiter::ArbiterForm::new(window, cx),
            notice: None,
            toast: None,
            pending: Vec::new(),
            rates: HashMap::new(),
            suggestions: HashMap::new(),
            placeholder_shown: crate::pages::chat::composer::PLACEHOLDER.to_owned(),
            suggestion_taken: None,
            git: None,
            repository: None,
            checks_open: HashMap::new(),
            touched: Vec::new(),
            review: crate::pages::review::Review::new(window, cx),
            focus: cx.focus_handle(),
            agents_trigger_focus: cx.focus_handle(),
            modal_focus: cx.focus_handle(),
            modal_scroll: ScrollHandle::new(),
            confirm_focus: cx.focus_handle(),
            file: crate::pages::file::FileView::new(),
            mcp_restarting: HashSet::new(),
            files: BTreeMap::new(),
            expanded: HashSet::new(),
            context: None,
            dock: crate::pages::chat::dock::Dock::new(cx),
            alerts: Default::default(),
            plan: crate::columns::inspector::plan::Plan::new(window, cx),
            providers_page: crate::pages::settings::providers::State::new(window, cx),
            secrets: None,
            voice: None,
            voice_models: None,
            voice_polling: false,
            web: None,
            web_busy: false,
            web_global: false,
            voice_form: crate::pages::settings::ai::voice::VoiceForm::new(window, cx),
            workshop: Default::default(),
            token_box: crate::pages::settings::TokenBox::new(window, cx),
            repositories: crate::pages::settings::Repositories::new(window, cx),
            limit_edit,
            context_open: HashSet::new(),
            context_load: Default::default(),
            team_page: Default::default(),
            overview: Default::default(),
            models_open: HashSet::new(),
            sent_pictures: HashMap::new(),
            last_input: std::time::Instant::now(),
            fog_cleared: None,
            fog_dawn: None,
            terminal: crate::columns::terminal::Panel::new(cx),
            plugins: crate::plugins::Plugins::new(window, cx),
            jobs: Vec::new(),
            sources: Vec::new(),
            settings: None,
            providers: Vec::new(),
            models_loading: false,
            models_pending: HashSet::new(),
            models_round: 0,
            picker: None,
            picker_closed: None,
            picked: Vec::new(),
            approval_open: Default::default(),
            attach_open: false,
            left_hidden: false,
            rail_hidden: false,
            quiet: false,
            flyout: None,
            flyout_inside: false,
            unfolded_modules: HashSet::new(),
            history: Default::default(),
            right_hidden: false,
            attachments: Vec::new(),
            pastes: Vec::new(),
            sent_rows: Vec::new(),
            rows_key: None,
            navigator_open: false,
            secret_inputs: Vec::new(),
            agent_path: Vec::new(),
            chat_menu: None,
            artifact_menu: None,
            update_ready: None,
            audio: Default::default(),
            dictation: None,
            synthesizing: false,
            update_tried: None,
            fading: Default::default(),
            path_menu: false,
            system: None,
            update: None,
            unfolded: HashSet::new(),
            list: ListState::new(0, ListAlignment::Top, px(800.)),
            rows: Vec::new(),
            reading_width: 720.,
            replied: None,
            diagram_hover: None,
            pins: Default::default(),
            list_chat: None,
            texts: Default::default(),
            measure_again: false,
            welcome: crate::pages::welcome::Welcome::new(window, cx),
        };
        workspace.pump_events(events, cx);
        workspace.pump_status(status, cx);
        workspace.watch_idle(cx);
        workspace.reload_projects(cx);
        workspace.reload_threads(cx);
        workspace.ask_welcome(cx);
        // The runs since the last look come up on the chat's "while you were away" card.
        workspace.load_schedules(cx);
        // The first start of a new version says what it brought. A first start ever has
        // nothing to compare with and only notes the version.
        let prefs = crate::ui::theme::prefs(cx);
        if prefs.seen_version != dusk_core::app::VERSION {
            let from = Some(prefs.seen_version.clone()).filter(|seen| !seen.is_empty());
            if from.is_some() || prefs.runs_seen_until != 0 {
                workspace.modal = Some(crate::modal::Modal::WhatsNew { from });
            }
            crate::ui::theme::change(cx, |prefs| {
                prefs.seen_version = dusk_core::app::VERSION.to_owned();
            });
        }
        if crate::ui::theme::prefs(cx).runs_seen_until == 0 {
            let now = chrono::Utc::now().timestamp_millis();
            crate::ui::theme::change(cx, |prefs| prefs.runs_seen_until = now);
        }
        workspace.load_about_me(cx);
        Self::answer_notifications(cx);
        window.focus(&workspace.focus, cx);
        workspace
    }
}
