//! A link to a chat: copied from the list of chats and pasted into a message, so the
//! agent can read that chat's messages.

/// What every chat link starts with; the thread id follows.
pub const THREAD_LINK_PREFIX: &str = "dusk://thread/";
/// Longer than any id the core makes (a UUID is 36).
const MAX_ID_BYTES: usize = 64;

/// The link to the thread `id`.
pub fn thread_link(id: &str) -> String {
    format!("{THREAD_LINK_PREFIX}{id}")
}

/// The thread ids of the chat links written in `text`, in order.
pub fn links_in(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices(THREAD_LINK_PREFIX)
        .filter_map(|(start, _)| {
            let rest = &text[start..];
            let end = rest[THREAD_LINK_PREFIX.len()..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .map_or(rest.len(), |at| at + THREAD_LINK_PREFIX.len());
            thread_of_link(&rest[..end])
        })
}

/// The thread id in a chat link, or a bare id; none for anything else. An id is letters,
/// digits, `-` and `_`, so a link cannot carry a path or a query.
pub fn thread_of_link(text: &str) -> Option<&str> {
    let text = text.trim();
    let id = text.strip_prefix(THREAD_LINK_PREFIX).unwrap_or(text);
    let plain = id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    (plain && (1..=MAX_ID_BYTES).contains(&id.len())).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_names_one_thread_and_nothing_else() {
        let id = "0192f3a4-5b6c-7d8e-9f00-112233445566";
        assert_eq!(thread_of_link(&thread_link(id)), Some(id));
        assert_eq!(thread_of_link(id), Some(id));
        let text = format!("see {}, and dusk://thread/b_2.", thread_link(id));
        assert_eq!(links_in(&text).collect::<Vec<_>>(), [id, "b_2"]);
        for bad in [
            "",
            "dusk://thread/",
            "dusk://thread/../x",
            "dusk://thread/a?b",
            "dusk://thread/a/b",
            "https://thread/a",
            &"a".repeat(65),
        ] {
            assert_eq!(thread_of_link(bad), None, "{bad}");
        }
    }
}
