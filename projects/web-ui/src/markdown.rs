//! Model output as markdown, drawn with DOM text nodes and elements only: there is no
//! HTML path, so what the model writes can only be text. Links open only if they are
//! http(s); pictures are never fetched, they show as their description.

use leptos::{html, prelude::*};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use web_sys::{Document, Element, Node};

#[component]
pub fn Markdown(#[prop(into)] text: Signal<String>) -> impl IntoView {
    let root = NodeRef::<html::Div>::new();
    Effect::new(move |_| {
        let source = text.get();
        if let Some(root) = root.get() {
            draw(&root, &source);
        }
    });
    view! { <div class="md" node_ref=root></div> }
}

/// Whether `url` may be followed from a message: http(s) only.
pub fn safe_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

fn draw(root: &Element, source: &str) {
    let Some(document) = root.owner_document() else {
        return;
    };
    root.set_text_content(None);
    let mut stack: Vec<Element> = vec![root.clone()];
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => {
                if let Some((outer, inner)) = open(&document, &tag) {
                    append(&stack, &outer);
                    stack.push(inner);
                }
            }
            Event::End(end) => {
                if closes_element(&end) && stack.len() > 1 {
                    stack.pop();
                }
            }
            Event::Text(text) => add_text(&document, &stack, &text),
            Event::Code(code) => {
                let element = make(&document, "code");
                element.set_text_content(Some(&code));
                append(&stack, &element);
            }
            // Raw HTML in a message is shown as the characters it is.
            Event::Html(html) | Event::InlineHtml(html) => add_text(&document, &stack, &html),
            Event::SoftBreak => add_text(&document, &stack, " "),
            Event::HardBreak => append(&stack, &make(&document, "br")),
            Event::Rule => append(&stack, &make(&document, "hr")),
            Event::TaskListMarker(done) => {
                add_text(&document, &stack, if done { "[x] " } else { "[ ] " });
            }
            _ => {}
        }
    }
}

fn make(document: &Document, tag: &str) -> Element {
    document.create_element(tag).expect("a known tag name")
}

fn append(stack: &[Element], child: &Element) {
    if let Some(parent) = stack.last() {
        let node: &Node = child.as_ref();
        let _ = parent.append_child(node);
    }
}

fn add_text(document: &Document, stack: &[Element], text: &str) {
    if let Some(parent) = stack.last() {
        let node = document.create_text_node(text);
        let _ = parent.append_child(&node);
    }
}

fn heading(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "h1",
        HeadingLevel::H2 => "h2",
        HeadingLevel::H3 => "h3",
        HeadingLevel::H4 => "h4",
        HeadingLevel::H5 => "h5",
        HeadingLevel::H6 => "h6",
    }
}

/// The element a tag opens, and the one inside it that its content goes to (a code block
/// is `pre > code`); none for a tag that is only a marker here.
fn open(document: &Document, tag: &Tag) -> Option<(Element, Element)> {
    let element = match tag {
        Tag::Paragraph => make(document, "p"),
        Tag::Heading { level, .. } => make(document, heading(*level)),
        Tag::BlockQuote(_) => make(document, "blockquote"),
        Tag::CodeBlock(kind) => {
            let pre = make(document, "pre");
            let code = make(document, "code");
            if let CodeBlockKind::Fenced(language) = kind {
                let language: String = language
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '+')
                    .collect();
                if !language.is_empty() {
                    let _ = code.set_attribute("data-lang", &language);
                }
            }
            let _ = pre.append_child(&code);
            return Some((pre, code));
        }
        Tag::List(Some(_)) => make(document, "ol"),
        Tag::List(None) => make(document, "ul"),
        Tag::Item => make(document, "li"),
        Tag::Table(_) => make(document, "table"),
        Tag::TableHead => make(document, "thead"),
        Tag::TableRow => make(document, "tr"),
        Tag::TableCell => make(document, "td"),
        Tag::Emphasis => make(document, "em"),
        Tag::Strong => make(document, "strong"),
        Tag::Strikethrough => make(document, "del"),
        Tag::Link { dest_url, .. } => {
            let link = make(document, "a");
            if safe_link(dest_url) {
                let _ = link.set_attribute("href", dest_url.trim());
                let _ = link.set_attribute("target", "_blank");
                let _ = link.set_attribute("rel", "noopener noreferrer nofollow");
            }
            link
        }
        Tag::Image { .. } => make(document, "span"),
        _ => return None,
    };
    Some((element.clone(), element))
}

fn closes_element(end: &TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image
    )
}
