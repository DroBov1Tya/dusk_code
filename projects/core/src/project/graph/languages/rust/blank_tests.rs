//! The lexer that wipes strings and comments is what keeps their text out of the index,
//! so each rule that decides where one starts or ends is held here by an example.

use proptest::prelude::*;

use super::blank::blank;

fn blanked(source: &str) -> String {
    String::from_utf8(blank(source.as_bytes())).expect("wiping keeps text valid")
}

#[test]
fn strings_and_comments_are_wiped_and_code_is_not() {
    for (source, wiped) in [
        // Comments: to the end of the line, or nested to the matching end.
        ("a // b \"c\n d", "a //     \n d"),
        ("/// doc\n//! in\nx", "//     \n//    \nx"),
        ("a /* b /* c */ d */ e", "a /*             */ e"),
        ("/** d */ /*! i */ /**/ x", "/*    */ /*    */ /**/ x"),
        ("a /* b\n c", "a /*  \n  "),
        ("a /*/ b */ c", "a /*    */ c"),
        // Strings: a backslash takes the next byte; a prefix changes nothing.
        (r#"a "b\"c" d"#, r#"a "    " d"#),
        (r#"a "b\\" d "e""#, r#"a "   " d " ""#),
        (r#"b"x" c"y" f("//z")"#, r#"b" " c" " f("   ")"#),
        ("a \"b\n c\" d", "a \" \n  \" d"),
        ("a \"b\\\n c\" d", "a \"  \n  \" d"),
        ("a \"b c", "a \"   "),
        ("\"a /* b\" c */ \"d\"", "\"      \" c */ \" \""),
        // Raw strings end at a quote with their own count of `#`.
        (r###"r"a\" b r#"c"d"# e"###, r###"r"  " b r#"   "# e"###),
        (
            r####"br##"a"#b"## cr#"c"# d"####,
            r####"br##"    "## cr#" "# d"####,
        ),
        (r###"r#"a" b"###, r###"r#"    "###),
        // `r` alone, a raw name and a longer word open nothing raw.
        (
            r###"r#fn(r, br) bar"x\"y" z"###,
            r###"r#fn(r, br) bar"    " z"###,
        ),
        (r###"xr#"a\"b"# c"###, r###"xr#"    "# c"###),
        // A character is not a string, and a quote in one opens none.
        (
            r#"('"', "a", '\'', "b", '\\', "c")"#,
            r#"('"', " ", '\'', " ", '\\', " ")"#,
        ),
        (
            r#"(b'"', "a", '\u{22}', "b", 'é', "c")"#,
            r#"(b'"', " ", '\u{22}', " ", 'é', " ")"#,
        ),
        (
            "('/', \"a\", '*', \"b\") // c",
            "('/', \" \", '*', \" \") //  ",
        ),
        // A lifetime is not a character.
        (
            r#"fn f<'a>(x: &'a str, y: &'static str) -> &'a str { "b" }"#,
            r#"fn f<'a>(x: &'a str, y: &'static str) -> &'a str { " " }"#,
        ),
        (
            r#"'outer: loop { break 'outer "a"; }"#,
            r#"'outer: loop { break 'outer " "; }"#,
        ),
        // A `'` that opens no whole character hides no string after it.
        ("'\\\n \"a\" '", "'\\\n \" \" '"),
        ("'\\x \"a\"\n\"b\"", "'\\x \" \"\n\" \""),
        ("'\\x \"a\" ' \"b\"", "'\\x \" \" ' \" \""),
        ("'# \"a\" ' 'ab' \"c\"", "'# \" \" ' 'ab' \" \""),
        // The line for the shell, but not an attribute of the crate.
        (
            "#!/bin/sh it's \"\nfn a() { \"b\" }",
            "#!              \nfn a() { \" \" }",
        ),
        ("#![doc = \"a\"]\n#! [b]", "#![doc = \" \"]\n#! [b]"),
        ("", ""),
    ] {
        assert_eq!(blanked(source), wiped, "{source:?}");
    }
}

/// Pieces that open, close and escape strings, characters and comments.
const PIECES: &[&str] = &[
    "\"", "'", "\\", "/", "*", "//", "/*", "*/", "#", "r", "b", "c", "r#", "!", "\n", " ", "a",
    "Zq9", "é", "(", ")", ";",
];

proptest! {
    #[test]
    fn wiping_moves_nothing_and_adds_nothing(source in proptest::collection::vec(any::<u8>(), 0..512)) {
        let wiped = blank(&source);
        prop_assert_eq!(wiped.len(), source.len());
        for (was, is) in source.iter().zip(&wiped) {
            prop_assert!(is == was || (*is == b' ' && *was != b'\n'));
        }
    }

    #[test]
    fn what_was_wiped_once_is_wiped(pieces in proptest::collection::vec(proptest::sample::select(PIECES), 0..80)) {
        // What is left reads as the same strings and comments: wiping moves no delimiter.
        let wiped = blank(pieces.concat().as_bytes());
        prop_assert_eq!(&blank(&wiped), &wiped);
    }

    #[test]
    fn the_text_of_a_string_or_a_comment_among_any_code_is_gone(
        before in proptest::collection::vec(proptest::sample::select(CLOSED), 0..12),
        held in proptest::sample::select(HOLDERS),
        after in proptest::collection::vec(proptest::sample::select(CLOSED), 0..12),
    ) {
        // Every piece before is closed, so the holder starts in code and its text is in
        // a string or a comment by the language's rules.
        let source = format!("{}{held}{}", before.concat(), after.concat());
        let wiped = blanked(&source);
        prop_assert!(!wiped.contains("Zq9"), "{source:?} -> {wiped:?}");
    }
}

/// Pieces of valid Rust that leave the lexer in code, the hard ones first.
const CLOSED: &[&str] = &[
    "'\"' ",
    "b'\"' ",
    "'\\'' ",
    "'\\\\' ",
    "'/' ",
    "'a ",
    "'static ",
    "\"\\\"\" ",
    "\"\\\\\" ",
    "\"//\" ",
    "\"/*\" ",
    "r\"\\\" ",
    "r#\"\"\"# ",
    "r#x ",
    "/* \" */ ",
    "/* /* ' */ */ ",
    "// \"\n",
    "/// '\n",
    "a / b ",
    "a * b ",
    "x.r ",
    "1.0 ",
    "\n",
    "fn a() {} ",
];

/// A string or a comment that holds `Zq9`, in each form the language has.
const HOLDERS: &[&str] = &[
    "\"Zq9\"",
    "\"a\\\"Zq9\"",
    "\"a\nZq9\"",
    "b\"Zq9\"",
    "c\"Zq9\"",
    "r\"Zq9\"",
    "r#\"a\"Zq9\"#",
    "br##\"a\"#Zq9\"##",
    "// Zq9\n",
    "/// Zq9\n",
    "//! Zq9\n",
    "/* Zq9 */",
    "/** a /* Zq9 */ */",
    "/*! a\nZq9 */",
];
