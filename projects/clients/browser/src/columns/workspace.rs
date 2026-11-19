//! Workspace chrome and responsive panels. Only presentation state lives here.

use super::{Sidebar, sidebar::chat_title};
use crate::{
    chat::inspector::Inspector,
    link::Conn,
    pages::{customize::Tab, settings::Page},
    state::{View, state},
    ui::{Icon, IconName},
};
use leptos::{ev, prelude::*};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, PartialEq)]
enum Drawer {
    Navigation,
    Inspector,
}

fn focus(id: &str) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = element.focus();
    }
}

fn viewport_width() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(1440.0)
}

impl Drawer {
    fn toggle(self) -> &'static str {
        match self {
            Self::Navigation => "navigation-toggle",
            Self::Inspector => "inspector-toggle",
        }
    }

    fn close_button(self) -> &'static str {
        match self {
            Self::Navigation => "navigation-close",
            Self::Inspector => "inspector-close",
        }
    }
}

#[component]
pub fn Workspace(children: Children) -> impl IntoView {
    let app = state();
    let drawer = RwSignal::new(None::<Drawer>);
    let width = RwSignal::new(viewport_width());
    let resize = window_event_listener(ev::resize, move |_| {
        let next = viewport_width();
        width.set(next);
        if matches!(drawer.get_untracked(), Some(Drawer::Navigation) if next >= 900.0)
            || matches!(drawer.get_untracked(), Some(Drawer::Inspector) if next >= 1280.0)
        {
            drawer.set(None);
            // At the docked breakpoint the button that opened the drawer is hidden.
            request_animation_frame(move || {
                if drawer.try_get_untracked() == Some(None) {
                    focus("workspace-title");
                }
            });
        }
    });
    on_cleanup(move || resize.remove());
    let close = move || {
        if let Some(previous) = drawer.get_untracked() {
            drawer.set(None);
            // The header becomes interactive after Leptos applies the new inert state.
            request_animation_frame(move || {
                if drawer.try_get_untracked() == Some(None) {
                    focus(previous.toggle());
                }
            });
        }
    };
    let open = move |panel| {
        drawer.set(Some(panel));
        request_animation_frame(move || {
            if drawer.try_get_untracked() == Some(Some(panel)) {
                focus(panel.close_button());
            }
        });
    };
    Effect::new(move |_| {
        app.view.get();
        close();
    });
    Effect::new(move |_| {
        if app.subagent.with(Option::is_some) && drawer.get_untracked().is_some() {
            drawer.set(None);
            request_animation_frame(|| focus("subagent-close"));
        }
    });
    let title = move || match app.view.get() {
        View::Chat(Some(id)) => app
            .chat(&id)
            .and_then(|chat| chat.with(|chat| chat.title.clone()))
            .or_else(|| {
                app.threads
                    .with(|threads| threads.iter().find(|t| t.id == id).map(chat_title))
            })
            .unwrap_or_else(|| "Chat".into()),
        View::Chat(None) => if app.temporary_next.get() {
            "Temporary chat"
        } else {
            "New chat"
        }
        .into(),
        View::Settings(_) => "Settings".into(),
        View::Customize(_) => "Customize".into(),
        View::Artifacts => "Artifacts".into(),
        View::Schedules => "Scheduled tasks".into(),
        View::Project(id) => app
            .projects
            .with(|projects| projects.iter().find(|p| p.id == id).map(|p| p.name.clone()))
            .unwrap_or_else(|| "Project".into()),
    };
    let connection = move || match app.conn.get() {
        Conn::Online => ("on", "Connected"),
        Conn::Connecting => ("wait", "Connecting"),
        Conn::Lost => ("off", "Reconnecting"),
    };
    let has_inspector = move || matches!(app.view.get(), View::Chat(Some(_)));
    let navigation_open = move || drawer.get() == Some(Drawer::Navigation);
    let inspector_open = move || drawer.get() == Some(Drawer::Inspector);
    let navigation_overlays = move || width.get() < 900.0;
    let inspector_overlays = move || width.get() < 1280.0;
    let keyboard = move |event: web_sys::KeyboardEvent| {
        let Some(panel) = drawer.get_untracked() else {
            return;
        };
        if event.key() == "Escape" {
            event.prevent_default();
            close();
            return;
        }
        if event.key() != "Tab" {
            return;
        }
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let selector = match panel {
            Drawer::Navigation => "#workspace-navigation",
            Drawer::Inspector => "#workspace-inspector",
        };
        let Some(root) = document.query_selector(selector).ok().flatten() else {
            return;
        };
        let Ok(nodes) = root.query_selector_all(
            "button:not([disabled]), input:not([disabled]), select:not([disabled]), \
             textarea:not([disabled]), a[href], summary, [tabindex='0']",
        ) else {
            return;
        };
        let visible: Vec<web_sys::HtmlElement> = (0..nodes.length())
            .filter_map(|i| nodes.item(i))
            .filter_map(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
            .filter(|e| e.offset_width() > 0 || e.offset_height() > 0)
            .collect();
        let (Some(first), Some(last)) = (visible.first(), visible.last()) else {
            return;
        };
        let active = document.active_element();
        if event.shift_key() && active.as_ref() == Some(first.as_ref()) {
            event.prevent_default();
            let _ = last.focus();
        } else if !event.shift_key() && active.as_ref() == Some(last.as_ref()) {
            event.prevent_default();
            let _ = first.focus();
        }
    };
    view! {
        <div class="app" class:has-inspector=has_inspector on:keydown=keyboard>
            <header class="workspace-header" inert=move || drawer.get().is_some()>
                <button id="navigation-toggle" class="icon navigation-toggle" aria-label="Open navigation"
                    aria-controls="workspace-navigation"
                    aria-expanded=move || if navigation_open() { "true" } else { "false" }
                    on:click=move |_| open(Drawer::Navigation)>
                    <Icon name=IconName::Menu />
                </button>
                <div class="workspace-brand" role="img" aria-label="Duskcode">
                    <img class="brand-emblem" src="mark.png" alt="" width="24" height="24" />
                    <span class="brand-name" aria-hidden="true"><b>"Dusk"</b>"code"</span>
                </div>
                <h1 id="workspace-title" class="workspace-title" tabindex="-1">{title}</h1>
                <span class="connection" role="status">
                    <span aria-hidden="true" class=move || format!("dot {}", connection().0)></span>
                    <span class="connection-label">{move || connection().1}</span>
                </span>
                <Show when=has_inspector>
                    <button id="inspector-toggle" class="icon inspector-toggle" aria-label="Open inspector"
                        aria-controls="workspace-inspector"
                        aria-expanded=move || if inspector_open() { "true" } else { "false" }
                        on:click=move |_| open(Drawer::Inspector)>
                        <Icon name=IconName::PanelRightOpen />
                    </button>
                </Show>
            </header>
            <div id="workspace-navigation" class="workspace-navigation" class:drawer-open=navigation_open
                role=move || navigation_overlays().then_some("dialog")
                aria-modal=move || navigation_open().then_some("true") aria-label="Workspace navigation"
                inert=move || (navigation_overlays() && !navigation_open()) || inspector_open()>
                <div class="drawer-heading">
                    <span>"Workspace"</span>
                    <button id="navigation-close" class="icon" aria-label="Close navigation" on:click=move |_| close()>
                        <Icon name=IconName::Close />
                    </button>
                </div>
                <nav class="rail" aria-label="Workspace">
                    <button class="icon" title="New chat" aria-label="New chat"
                        on:click=move |_| app.new_chat(None)>
                        <Icon name=IconName::Plus />
                    </button>
                    <div class="rail-divider"></div>
                    <RailButton label="Chats" icon=IconName::Inbox target=View::Chat(None) />
                    <RailButton label="Scheduled tasks" icon=IconName::Calendar target=View::Schedules />
                    <RailButton label="Customize" icon=IconName::Blocks target=View::Customize(Tab::Agents) />
                    <RailButton label="Artifacts" icon=IconName::LayoutDashboard target=View::Artifacts />
                    <div class="grow"></div>
                    <button class="icon" title="Temporary chat" aria-label="Temporary chat"
                        on:click=move |_| app.new_temporary_chat()>
                        <Icon name=IconName::Temporary />
                    </button>
                    <RailButton label="Settings" icon=IconName::Settings target=View::Settings(Page::Appearance) />
                </nav>
                <Sidebar />
            </div>
            <div class="workspace-content" inert=move || drawer.get().is_some()>{children()}</div>
            <Show when=has_inspector>
                <div id="workspace-inspector" class="workspace-inspector" class:drawer-open=inspector_open
                    role=move || inspector_overlays().then_some("dialog")
                    aria-modal=move || inspector_open().then_some("true") aria-label="Chat inspector"
                    inert=move || (inspector_overlays() && !inspector_open()) || navigation_open()>
                    <div class="inspector-heading">
                        <span>"Inspector"</span>
                        <button id="inspector-close" class="icon" aria-label="Close inspector" on:click=move |_| close()>
                            <Icon name=IconName::PanelRightClose />
                        </button>
                    </div>
                    <Inspector />
                </div>
            </Show>
            <Show when=move || {
                matches!(drawer.get(), Some(Drawer::Navigation) if navigation_overlays())
                    || matches!(drawer.get(), Some(Drawer::Inspector) if inspector_overlays())
            }>
                <div class="workspace-scrim" on:click=move |_| close()></div>
            </Show>
        </div>
    }
}

#[component]
fn RailButton(label: &'static str, icon: IconName, target: View) -> impl IntoView {
    let app = state();
    let selected = {
        let target = target.clone();
        move || std::mem::discriminant(&app.view.get()) == std::mem::discriminant(&target)
    };
    let navigate = move |_| {
        let already_in_chat = matches!(target, View::Chat(_))
            && app
                .view
                .with_untracked(|view| matches!(view, View::Chat(_)));
        if !already_in_chat {
            app.show(target.clone());
        }
    };
    view! {
        <button class="icon" class:active=selected.clone() aria-current=move || selected().then_some("page")
            title=label aria-label=label on:click=navigate>
            <Icon name=icon />
        </button>
    }
}
