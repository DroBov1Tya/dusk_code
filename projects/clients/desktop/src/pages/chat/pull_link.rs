//! A link in a chat to a pull request of the shown project's own repository is
//! drawn as a chip, its state icon with `owner/repo#n · title`, and under the pointer
//! a card says who opened it, when, how much it changes and where it stands. The core
//! gives the data: the pull requests it watches, else one it reads for the asking;
//! nothing is fetched here. Until it answers, and for any other repository or host,
//! the link is drawn as before. A click opens the page as any link does.

use std::{collections::HashMap, sync::LazyLock, time::Duration};

use dusk_protocol::project::{PullState, PullStatus, RepositoryStatus, pull_number};
use gpui_kit::{
    component::{
        ActiveTheme,
        hover_card::HoverCard,
        text::{
            InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode,
            MarkdownParseContext, MarkdownPlugin, markdown_ast,
        },
    },
    *,
};

use crate::{
    columns::inspector::{checks, pulls},
    ui::{Type, avatar, widgets},
    workspace::Workspace,
};

const NODE: &str = "dusk-pull";
/// Longer titles are cut in the chip; the card has the whole.
const CHIP_TITLE_CHARS: usize = 48;

/// What the chat knows of the shown project's pull requests, beside the window's state
/// so the text view can read it while it lays a line out.
#[derive(Default)]
pub struct PullLinks {
    project: Option<String>,
    /// The shown project's repository, as its forge names it, for the chip's label.
    repository: String,
    /// What every pull request's page starts with; none while no forge serves it.
    prefix: Option<String>,
    listed: Vec<PullStatus>,
    /// Read for a link: `None` while asked, or when the core had none to give.
    looked: HashMap<u64, Option<PullStatus>>,
}

impl Global for PullLinks {}

impl PullLinks {
    fn pull(&self, number: u64) -> Option<&PullStatus> {
        self.listed
            .iter()
            .find(|pull| pull.number == number)
            .or_else(|| self.looked.get(&number)?.as_ref())
    }
}

/// The text view's plugins with this one: `marked` with the answer's labels too. Built
/// once, so the views keep their parsed documents across frames.
pub fn extensions(marked: bool) -> MarkdownExtensions {
    static PLAIN: LazyLock<MarkdownExtensions> =
        LazyLock::new(|| MarkdownExtensions::default().plugin(PullLink));
    static MARKED: LazyLock<MarkdownExtensions> =
        LazyLock::new(|| super::marks::extensions().plugin(PullLink));
    if marked {
        MARKED.clone()
    } else {
        PLAIN.clone()
    }
}

/// The shown project's repository status changed: the chips follow it.
pub fn show(ws: &Workspace, cx: &mut App) {
    let links = cx.default_global::<PullLinks>();
    let served = ws
        .repository
        .as_ref()
        .and_then(|status: &RepositoryStatus| status.forge.served());
    // A link read under another prefix may be another repository's.
    let prefix = served
        .map(|served| served.pull_prefix.clone())
        .filter(|prefix| !prefix.is_empty());
    if links.project != ws.project || links.prefix != prefix {
        links.looked.clear();
    }
    links.project = ws.project.clone();
    links.repository = served
        .map(|served| served.repository.clone())
        .unwrap_or_default();
    links.prefix = prefix;
    links.listed = served
        .map(|served| served.pulls.clone())
        .unwrap_or_default();
    cx.refresh_windows();
}

/// The link's text is its own address, as a bare address in a message is: that text is
/// what the chip stands for. A link with words of its own is left as it is.
struct PullLink;

impl MarkdownPlugin for PullLink {
    fn name(&self) -> &str {
        NODE
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Text(text) = node else {
            return None;
        };
        let url = text.value.trim();
        // Any https address with a number in its path, as the browser client chips
        // them; the core's prefix decides in `render_inline` which are pull requests.
        let path = url.split(['#', '?']).next().unwrap_or_default();
        (url.starts_with("https://")
            && url.len() <= 300
            && path
                .split('/')
                .any(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())))
        .then(|| MarkdownNode::new(NODE, url.to_owned()).text(text.value.clone()))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        _context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let url = node.data::<String>()?;
        let links = cx.try_global::<PullLinks>()?;
        let prefix = links.prefix.clone()?;
        let number = pull_number(url, &prefix)?;
        if let Some(pull) = links.pull(number) {
            return Some(InlineElement::new(chip(&links.repository, pull, cx)));
        }
        if !links.looked.contains_key(&number) {
            cx.default_global::<PullLinks>().looked.insert(number, None);
            window.defer(cx, move |window, cx| {
                if let Some(ws) = window.root::<Workspace>().flatten() {
                    ws.update(cx, |ws, cx| ws.look_up_pull(number, cx));
                }
            });
        }
        // Drawn by the text view as the link it is.
        None
    }
}

/// The chip in the line, with the card under the pointer.
fn chip(github: &str, pull: &PullStatus, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mut title: String = pull.title.chars().take(CHIP_TITLE_CHARS).collect();
    if pull.title.chars().count() > CHIP_TITLE_CHARS {
        title.push('\u{2026}');
    }
    let trigger = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_1p5()
        .rounded_md()
        .bg(theme.muted)
        // The link's underline and color are inherited; the chip is its own.
        .text_decoration_0()
        .text_color(theme.foreground)
        .child(widgets::icon(
            pulls::state_icon(pull.state),
            pulls::state_color(pull.state, cx),
        ))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(format!("{github}#{}", pull.number)),
        )
        .child(div().text_color(theme.muted_foreground).child("\u{b7}"))
        .child(title);
    let (github, pull) = (github.to_owned(), pull.clone());
    HoverCard::new(SharedString::from(format!("pull-link-{}", pull.number)))
        .anchor(Anchor::TopLeft)
        .open_delay(Duration::from_millis(300))
        .trigger(trigger)
        .content(move |_, _, cx| card(&github, &pull, cx))
        .into_any_element()
}

/// The title, the repository and number, who and when and how much, and the state.
fn card(github: &str, pull: &PullStatus, cx: &App) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let dot = || div().text_color(muted).child("\u{b7}");
    let mut facts = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_1p5()
        .small()
        .text_color(muted);
    if !pull.author.is_empty() {
        facts = facts
            .child(avatar::avatar(&pull.author, 18., None, cx).rounded_full())
            .child(
                div()
                    .text_color(theme.foreground)
                    .child(pull.author.clone()),
            );
    }
    if let Some(created) = pull.created {
        facts = facts
            .child(dot())
            .child(format!("{} ago", checks::ago(checks::now() - created)));
    }
    facts = facts
        .child(dot())
        .child(
            div()
                .text_color(theme.success)
                .child(format!("+{}", pulls::grouped(pull.added))),
        )
        .child(
            div()
                .text_color(theme.danger)
                .child(format!("\u{2212}{}", pulls::grouped(pull.removed))),
        );
    if let Some(files) = pull.files {
        facts = facts
            .child(dot())
            .child(widgets::count(files as usize, "file"));
    }
    let color = pulls::state_color(pull.state, cx);
    div()
        .w(px(320.))
        .flex()
        .flex_col()
        .gap_1p5()
        .p_1()
        // The card is drawn inside the link and would take its underline and color.
        .text_decoration_0()
        .text_color(theme.foreground)
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(pull.title.clone()),
        )
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(format!("{github} #{}", pull.number)),
        )
        .child(facts)
        .child(
            div().flex().flex_row().child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .bg(color.opacity(0.14))
                    .text_color(color)
                    .small()
                    .font_weight(FontWeight::MEDIUM)
                    .child(widgets::icon(pulls::state_icon(pull.state), color))
                    .child(state_word(pull.state)),
            ),
        )
}

fn state_word(state: PullState) -> &'static str {
    match state {
        PullState::Open => "Open",
        PullState::Draft => "Draft",
        PullState::Merged => "Merged",
        PullState::Closed => "Closed",
    }
}

impl Workspace {
    /// Asks the core once for pull request `number` of the shown project, for a link
    /// that names it; the chips are drawn again with the answer.
    fn look_up_pull(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(id) = cx
            .try_global::<PullLinks>()
            .and_then(|links| links.project.clone())
        else {
            return;
        };
        let asked = id.clone();
        self.core.call(
            cx,
            move |client| async move { client.projects().pull_status(&asked, number).await },
            move |_, result, cx| {
                let links = cx.default_global::<PullLinks>();
                if links.project.as_deref() != Some(id.as_str()) {
                    return;
                }
                links.looked.insert(number, result.ok().flatten());
                cx.refresh_windows();
            },
        );
    }
}
