/// The first `max` characters of `text`, and `mark` after them when that cut something.
pub fn clip_chars(text: &str, max: usize, mark: &str) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}{mark}", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_characters_and_marks_only_a_cut() {
        assert_eq!(clip_chars("héllo", 5, "…"), "héllo");
        assert_eq!(clip_chars("héllo", 2, "…"), "hé…");
        assert_eq!(clip_chars("héllo", 0, ""), "");
        assert_eq!(clip_chars("", 3, "…"), "");
    }
}
