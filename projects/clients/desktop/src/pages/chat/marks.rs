//! Coloured labels in an answer: `{{tone: label}}`, found by the protocol's parser in the
//! prose, become a link of our own scheme, and a markdown plugin draws that link as a
//! label in the tone. The label is a plain box, never a link, so it opens nothing.

use dusk_protocol::thread::{Span, Tone, marks};
use gpui_kit::{
    component::text::{
        InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
        MarkdownPlugin, markdown_ast,
    },
    *,
};

use crate::ui::theme;

/// The scheme of a label in the markdown the text view parses. Any the model wrote itself
/// is cut before (`links::linkify`), so only a label of the parser can carry it.
pub const SCHEME: &str = "dusk-mark:";
const NODE: &str = "dusk-mark";

/// The plugin, built once so the text view keeps its parsed document across frames.
pub fn extensions() -> MarkdownExtensions {
    static EXTENSIONS: std::sync::LazyLock<MarkdownExtensions> =
        std::sync::LazyLock::new(|| MarkdownExtensions::default().plugin(Label));
    EXTENSIONS.clone()
}

/// Writes prose with each label as `[words](dusk-mark:tone)`. A `!` just before would open
/// a picture, so an unescaped one is escaped.
pub fn encode(out: &mut String, prose: &str) {
    for span in marks(prose) {
        match span {
            Span::Text(text) => out.push_str(text),
            Span::Mark { tone, text } => {
                let slashes = out
                    .strip_suffix('!')
                    .map(|before| before.len() - before.trim_end_matches('\\').len());
                if slashes.is_some_and(|slashes| slashes % 2 == 0) {
                    out.insert(out.len() - 1, '\\');
                }
                out.push_str(&format!("[{text}]({SCHEME}{})", tone.name()));
            }
        }
    }
}

struct Label;

impl MarkdownPlugin for Label {
    fn name(&self) -> &str {
        NODE
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else {
            return None;
        };
        let name = link.url.strip_prefix(SCHEME)?;
        let tone = Tone::ALL.into_iter().find(|tone| tone.name() == name)?;
        let [markdown_ast::Node::Text(words)] = link.children.as_slice() else {
            return None;
        };
        Some(
            MarkdownNode::new(NODE, tone)
                .text(words.value.clone())
                .markdown(format!("{{{{{name}: {}}}}}", words.value)),
        )
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let tone = *node.data::<Tone>()?;
        let (color, background) = colors(tone, cx);
        Some(InlineElement::new(
            div()
                .px(px(6.))
                .rounded(px(4.))
                .bg(background)
                .text_color(color)
                .text_size(context.font_size() * 0.86)
                .font_weight(FontWeight::MEDIUM)
                .child(SharedString::from(node.as_text().to_owned())),
        ))
    }
}

/// The text and the fill of a tone, as the browser has them: the tone mixed into the text
/// color, on the tone at 14%; neutral is the secondary text on the raised surface.
fn colors(tone: Tone, cx: &App) -> (Hsla, Hsla) {
    let palette = theme::palette(cx);
    let hex = |value: &str| -> Hsla {
        Rgba::try_from(value).map_or_else(|_| gpui::transparent_black(), Into::into)
    };
    let hue = match tone {
        Tone::Neutral => return (hex(palette.secondary), hex(palette.raised)),
        Tone::Info => hex(theme::function_color(palette.dark)),
        Tone::Success => hex(palette.success),
        Tone::Warning => hex(palette.warning),
        Tone::Danger => hex(palette.danger),
        Tone::Accent => hex(palette.accent),
    };
    let text = hex(palette.foreground).blend(hue.opacity(0.65));
    (text, hue.opacity(0.14))
}
