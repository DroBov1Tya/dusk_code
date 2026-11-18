//! The parser against its limits. Source is untrusted input, so beside the fixture's
//! exact outline these hold every cap, the time limit, and that no input panics or puts
//! the text of a string or a comment into a name.

use std::{cell::Cell, fmt::Write, time::Instant};

use proptest::prelude::*;

use super::{
    outline::{ImportPath, Name, Outline},
    parse::parse_until,
    *,
};

const FIXTURE: &str = include_str!("fixtures/library.rs");

fn rust(source: impl AsRef<[u8]>) -> Result<Outline, Skipped> {
    parse(Language::Rust, "core/src/library.rs", source.as_ref())
        .expect("the Rust grammar loads and gives a tree")
        .outline
}

/// One line a symbol (`kind parent name first-last`), then one line a ref
/// (`kind name line xUses path`); `-` where there is none.
fn written(outline: &Outline) -> String {
    let mut text = String::new();
    for symbol in &outline.symbols {
        let parent = symbol.parent.as_ref().map_or("-", Name::as_str);
        let (kind, name) = (symbol.kind.as_str(), symbol.name.as_str());
        writeln!(
            text,
            "{kind} {parent} {name} {}-{}",
            symbol.line, symbol.end_line
        )
        .unwrap();
    }
    for found in &outline.refs {
        let path = found.path.as_ref().map_or("-", ImportPath::as_str);
        let (kind, name) = (found.kind.as_str(), found.name.as_str());
        writeln!(text, "{kind} {name} {} x{} {path}", found.line, found.uses).unwrap();
    }
    text
}

fn names(outline: &Outline) -> Vec<&str> {
    let symbols = outline.symbols.iter().flat_map(|symbol| {
        [
            Some(symbol.name.as_str()),
            symbol.parent.as_ref().map(Name::as_str),
        ]
    });
    let refs = outline.refs.iter().flat_map(|found| {
        [
            Some(found.name.as_str()),
            found.path.as_ref().map(ImportPath::as_str),
        ]
    });
    symbols.chain(refs).flatten().collect()
}

#[test]
fn the_fixture_gives_its_symbols_and_refs() {
    let outline = rust(FIXTURE).unwrap();
    assert_eq!(written(&outline), include_str!("fixtures/library.outline"));
}

#[test]
fn a_path_gives_the_language_the_module_and_the_test_mark() {
    for (path, module, is_test) in [
        (
            "projects/core/src/project/graph/mod.rs",
            "project::graph",
            false,
        ),
        (
            "projects/core/src/project/graph/parse.rs",
            "project::graph::parse",
            false,
        ),
        (
            "projects/core/src/project/graph/tests.rs",
            "project::graph::tests",
            true,
        ),
        (
            "core/src/runtime/repeats/signature_tests.rs",
            "runtime::repeats::signature_tests",
            true,
        ),
        ("src/lib.rs", "", false),
        ("src/main.rs", "", false),
        ("tests/it/main.rs", "tests::it", true),
        ("build.rs", "build", false),
        ("src/contests/latest.rs", "contests::latest", false),
    ] {
        let language = Language::of(path).unwrap();
        assert_eq!(language.as_str(), "rust");
        let parsed = parse(language, path, b"").unwrap();
        assert_eq!(
            (parsed.module.as_str(), parsed.is_test),
            (module, is_test),
            "{path}"
        );
        assert_eq!(parsed.outline, Ok(Outline::default()));
    }
    for other in [
        "README.md",
        "Cargo.toml",
        "rs",
        "src/rs",
        "a.rs/b",
        "a.RS",
        "a.rs.bak",
        "",
    ] {
        assert_eq!(Language::of(other), None, "{other}");
    }
}

#[test]
fn the_text_of_a_string_or_a_comment_is_never_a_name() {
    let source = r##"
        const KEY: &str = "sk-ant-api03-Zm9vYmFyQUtJQUlPU0ZPRE5ON0VYQU1QTEU";
        static AWS: &[u8] = b"AKIAIOSFODNN7EXAMPLE";
        // call_in_comment(); TypeInComment::new()
        /* AKIAIOSFODNN7COMMENT /* nested TypeInBlock */ */
        /// TypeInDoc::from(AKIAIOSFODNN7DOCDOC)
        fn open() -> Client {
            let raw = r#"InRaw::call() "quoted" AKIAIOSFODNN7RAWRAW"#;
            connect!("sk_live_InMacro", 'S', format!("Bearer {}", "AKIAIOSFODNN7MACROS"))
        }
    "##;
    let outline = rust(source).unwrap();
    assert_eq!(
        written(&outline),
        "constant - KEY 2-2\nconstant - AWS 3-3\nfunction - open 7-10\n\
         use Client 7 x1 -\nuse connect 9 x1 -\nuse format 9 x1 -\n"
    );
}

#[test]
fn nothing_is_kept_after_a_string_that_never_closes() {
    // The parser reads the rest of such a file as code, and a key in it as a name.
    let source = "fn before() { Kept::call(); }\n\
                  fn broken() { let key = \"AKIAIOSFODNN7EXAMPLE secret_call(); Leaked::now(); }\n\
                  fn after() { AlsoLeaked::now(); }\n";
    let outline = rust(source).unwrap();
    assert_eq!(
        written(&outline),
        "function - before 1-1\nfunction - broken 2-2\nuse Kept 1 x1 -\nuse call 1 x1 -\n"
    );
    // The same for raw strings, whose text holds quotes of its own.
    for opener in ["r#\"", "r##\"", "br#\"", "r\""] {
        let source = format!(
            "fn broken() {{ let json = {opener}{{\"key\": \"AKIAIOSFODNN7EXAMPLE\"}}\"; Leaked::now(); }}\n\
             fn after() {{ AlsoLeaked::now(); }}\n"
        );
        let outline = rust(&source).unwrap();
        let kept = names(&outline);
        assert!(
            kept.iter().all(|name| ["broken", "json"].contains(name)),
            "{opener} {kept:?}"
        );
    }
}

#[test]
fn nothing_is_kept_from_a_block_comment_that_never_closes() {
    // A comment that runs to the end of the file holds whatever was written there, a
    // password among it; none of it may become a name.
    for opener in ["/*", "/**", "/*!", "/* /*", "/* outer /* inner */"] {
        for rest in [
            " Hunter2Prod Leaked::now(); fn leaked_fn() {}\nstruct LeakedType;\n",
            " password = Hunter2Prod; \"quoted\" Leaked::now()\n",
            "\nfn leaked_fn() { Hunter2Prod::call(); }\n",
        ] {
            let source = format!("fn before() {{ Kept::call(); }}\n{opener}{rest}");
            let outline = rust(&source).unwrap();
            let kept = names(&outline);
            assert!(
                kept.contains(&"before")
                    && kept
                        .iter()
                        .all(|name| ["before", "Kept", "call"].contains(name)),
                "{opener:?} {rest:?}: {kept:?}"
            );
        }
    }
}

#[test]
fn a_file_with_a_syntax_error_gives_what_parsed() {
    let source = "struct Kept;\nfn broken( { let = ; }\nimpl Kept { fn method(&self) {} }\n";
    let outline = rust(source).unwrap();
    let kept: Vec<&str> = outline
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert!(
        kept.contains(&"Kept") && kept.contains(&"method"),
        "{kept:?}"
    );
    // Bytes that are not text at all: no panic, and the names around them are still read.
    let outline = rust(b"fn a() {}\n\xff\xfe\x00\xc3\x28\nfn b\xff() {}\nfn c() {}\n").unwrap();
    let kept: Vec<&str> = outline
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert!(kept.contains(&"a") && kept.contains(&"c"), "{kept:?}");
    assert!(names(&outline).iter().all(|name| Name::new(name).is_some()));
}

#[test]
fn a_file_past_a_cap_is_skipped_and_one_at_the_cap_is_not() {
    let blank = |bytes| vec![b'\n'; bytes];
    assert_eq!(rust(blank(MAX_FILE_BYTES)), Ok(Outline::default()));
    assert_eq!(rust(blank(MAX_FILE_BYTES + 1)), Err(Skipped::TooLarge));

    let line = |bytes: usize| format!("fn a() {{}}\n//{}\nfn b() {{}}", "x".repeat(bytes - 2));
    assert_eq!(rust(line(5_000)).unwrap().symbols.len(), 2);
    assert_eq!(rust(line(5_001)), Err(Skipped::LongLines));

    let functions = |count| {
        (0..count)
            .map(|n| format!("fn f{n}() {{}}\n"))
            .collect::<String>()
    };
    assert_eq!(rust(functions(2_000)).unwrap().symbols.len(), 2_000);
    assert_eq!(rust(functions(2_001)), Err(Skipped::TooMany));

    // Impls are not symbols; without a cap of their own a file could hold 50,000.
    let impls = |count| "impl A for B {}\n".repeat(count);
    assert_eq!(rust(impls(4_000)).unwrap().symbols.len(), 0);
    assert_eq!(rust(impls(4_001)), Err(Skipped::TooMany));
}

#[test]
fn names_referred_to_past_the_cap_are_dropped() {
    let calls: String = (0..4_100).map(|n| format!("    call{n}();\n")).collect();
    let source = format!("use a::{{b, c}};\nfn all() {{\n{calls}    call0();\n}}\n");
    let outline = rust(source).unwrap();
    assert_eq!(outline.refs.len(), 4_000);
    // The imports and the first uses are the ones kept, and a kept name still counts.
    let has = |name: &str| {
        outline
            .refs
            .iter()
            .find(|found| found.name.as_str() == name)
    };
    assert!(has("b").is_some() && has("c").is_some());
    assert_eq!(has("call0").map(|found| found.uses), Some(2));
    assert!(has("call3997").is_some() && has("call3998").is_none());
}

#[test]
fn a_name_is_an_identifier_of_128_characters_at_most() {
    for name in ["a", "_", "größe", "Список", "x1", &"a".repeat(128)] {
        assert!(Name::new(name).is_some(), "{name}");
    }
    for name in [
        "",
        "a b",
        "a-b",
        "a.b",
        "r#type",
        "a\n",
        "a\u{202e}b",
        "\"a\"",
        &"a".repeat(129),
    ] {
        assert!(Name::new(name).is_none(), "{name}");
    }
    assert!(ImportPath::new(&"a".repeat(200)).is_some());
    for path in ["", "a b", "a;b", "a\"b", &"a".repeat(201)] {
        assert!(ImportPath::new(path).is_none(), "{path}");
    }

    let long = "a".repeat(129);
    let source = format!(
        "fn {short}() {{}}\nfn {long}() {{}}\nuse m::{long};\nuse {deep}::Far;\nfn r#try() {{}}\n",
        short = "a".repeat(128),
        deep = "m::".repeat(80),
    );
    let outline = rust(source).unwrap();
    assert_eq!(
        written(&outline),
        format!("function - {} 1-1\n", "a".repeat(128))
    );
}

#[test]
fn a_parse_out_of_time_is_too_slow() {
    // The clock is counted, not read, so the test does not depend on the machine: the
    // time is up at the `after`-th look. Small numbers stop the parser, large ones the walk.
    let source = FIXTURE.repeat(12);
    let mut stopped_at = Vec::new();
    for after in [0, 1, 2, 10, 100, 1_000, 10_000, 100_000, u32::MAX] {
        let looks = Cell::new(0_u32);
        let late = || {
            looks.set(looks.get() + 1);
            looks.get() > after
        };
        let parsed = parse_until(Language::Rust, "a.rs", source.as_bytes(), &late).unwrap();
        if parsed.outline.is_ok() {
            // Enough looks to finish: every smaller number must have stopped it.
            assert!(looks.get() <= after);
            break;
        }
        assert_eq!(parsed.outline, Err(Skipped::TooSlow), "after {after} looks");
        // It stops where it is told to: one more look at most, after the parser returns.
        assert!(
            looks.get() <= after + 2,
            "{} looks, late after {after}",
            looks.get()
        );
        stopped_at.push(after);
    }
    assert!(
        stopped_at.len() >= 4,
        "the parser never asked for the time: {stopped_at:?}"
    );
}

#[test]
fn shapes_made_to_be_slow_or_deep_end_in_time() {
    // Each is built to cost the most at one step: the parser's recovery, the cursor in a
    // deep or a wide node, the walk of a `use`, the names counted, the matches in flight.
    // At the full size most run out of time; at an eighth they reach the later steps.
    for room in [MAX_FILE_BYTES / 8, MAX_FILE_BYTES - 32] {
        let fill = |piece: &str| piece.repeat(room / piece.len());
        let nested = |open: &str, close: &str| {
            let depth = room / (open.len() + close.len());
            format!("{}{}", open.repeat(depth), close.repeat(depth))
        };
        let shapes = [
            ("names", fill("A;\n")),
            ("calls", fill("a();B::c();\n")),
            ("open braces", fill("{\n")),
            ("blocks", format!("fn a() {{ {} }}", nested("{\n", "}\n"))),
            (
                "parentheses",
                format!("fn a() {{ {} }}", nested("(\n", ")\n")),
            ),
            ("types", format!("type A = {};", nested("B<\n", ">\n"))),
            ("modules", nested("mod a {\n", "}\n")),
            ("use lists", format!("use {};", nested("a::{\n", "}\n"))),
            ("wide use", format!("use a::{{{}}};", fill("b,\n"))),
            ("errors", fill(") \" ]\n")),
            ("attributes", format!("{}fn a() {{}}", fill("#[test]\n"))),
            ("impls", fill("impl A {}\n")),
        ];
        for (shape, source) in shapes {
            let started = Instant::now();
            let outline = rust(&source);
            let took = started.elapsed();
            let ended = outline.as_ref().map(|outline| outline.symbols.len());
            println!("{shape}, {} bytes: {took:?}, {ended:?}", source.len());
            holds(&outline).unwrap();
            // The budget is 250 ms and the parser looks at the clock between steps, not
            // within one; the rest of this allowance is for a machine busy with other work.
            assert!(took.as_secs() < 5, "{shape} took {took:?}");
        }
    }
}

/// What must hold for the outline of any input at all.
fn holds(outline: &Result<Outline, Skipped>) -> Result<(), TestCaseError> {
    let Ok(outline) = outline else {
        return Ok(());
    };
    prop_assert!(outline.symbols.len() <= 2_000);
    prop_assert!(outline.refs.len() <= 4_000);
    for symbol in &outline.symbols {
        prop_assert!(
            Name::new(symbol.name.as_str()).is_some(),
            "{:?}",
            symbol.name
        );
        if let Some(parent) = &symbol.parent {
            prop_assert!(Name::new(parent.as_str()).is_some(), "{parent:?}");
        }
        prop_assert!(
            symbol.line >= 1 && symbol.end_line >= symbol.line,
            "{symbol:?}"
        );
    }
    let mut keys = std::collections::HashSet::new();
    for found in &outline.refs {
        prop_assert!(Name::new(found.name.as_str()).is_some(), "{:?}", found.name);
        prop_assert!(found.line >= 1 && found.uses >= 1, "{found:?}");
        if let Some(path) = found.path.as_ref() {
            prop_assert!(ImportPath::new(path.as_str()).is_some(), "{path:?}");
        }
        // The key of the table: one row a name and kind.
        prop_assert!(
            keys.insert((found.kind.as_str(), found.name.as_str())),
            "{found:?}"
        );
    }
    Ok(())
}

/// Pieces of Rust that, put in any order, reach the parser's recovery far more often
/// than random bytes do.
const TOKENS: &[&str] = &[
    "fn ",
    "impl ",
    "trait ",
    "struct ",
    "enum ",
    "mod ",
    "use ",
    "const ",
    "static ",
    "type ",
    "macro_rules! ",
    "pub ",
    "for ",
    "as ",
    "self",
    "super",
    "crate",
    "a",
    "B",
    "Cc",
    "_d",
    "::",
    "{",
    "}",
    "(",
    ")",
    "<",
    ">",
    "[",
    "]",
    ";",
    ",",
    ":",
    "=",
    "*",
    "&",
    "!",
    "#",
    ".",
    "->",
    "#[test]",
    "#[a::test]",
    "\"",
    "'",
    "r#\"",
    "\"#",
    "//",
    "/*",
    "*/",
    "\\",
    "\n",
    " ",
    "0",
    "'a",
    "b'",
    "é",
    "\u{0}",
    "\u{202e}",
];

proptest! {
    #[test]
    fn any_bytes_parse_within_the_caps(source in proptest::collection::vec(any::<u8>(), 0..4_096)) {
        holds(&rust(&source))?;
    }

    #[test]
    fn any_order_of_tokens_parses_within_the_caps(
        tokens in proptest::collection::vec(proptest::sample::select(TOKENS), 0..600),
    ) {
        holds(&rust(tokens.concat()))?;
    }

    #[test]
    fn the_fixture_cut_and_spliced_anywhere_parses_within_the_caps(
        edits in proptest::collection::vec(
            (any::<prop::sample::Index>(), any::<prop::sample::Index>(),
             proptest::collection::vec(any::<u8>(), 0..24)),
            1..8,
        ),
    ) {
        // Each edit replaces a run of bytes with a few random ones, wherever it falls.
        let mut source = FIXTURE.as_bytes().to_vec();
        for (from, length, with) in edits {
            let from = from.index(source.len());
            let to = (from + length.index(64)).min(source.len());
            source.splice(from..to, with);
        }
        holds(&rust(&source))?;
    }

    #[test]
    fn the_markers_in_strings_and_comments_stay_out_whatever_breaks_around_them(
        edits in proptest::collection::vec(
            (0..4_u8, any::<prop::sample::Index>(),
             proptest::collection::vec(any::<u8>().prop_filter("no quote, one line", |byte| {
                 !matches!(byte, b'"' | b'\n')
             }), 0..40)),
            1..12,
        ),
    ) {
        // Whole lines are dropped, doubled, moved and broken up by junk with no quote in
        // it, so every string and comment of the fixture stays whole while the code
        // around it stops making sense.
        let mut lines: Vec<Vec<u8>> = FIXTURE.lines().map(|line| line.as_bytes().to_vec()).collect();
        for (edit, at, junk) in edits {
            let at = at.index(lines.len());
            match edit {
                0 => drop(lines.remove(at)),
                1 => lines.insert(at, lines[at].clone()),
                2 => lines.swap(0, at),
                _ => lines.insert(at, junk),
            }
            if lines.is_empty() {
                lines.push(Vec::new());
            }
        }
        let outline = rust(lines.join(&b'\n'));
        holds(&outline)?;
        if let Ok(outline) = &outline {
            for name in names(outline) {
                prop_assert!(!name.contains("MARKER") && !name.contains("4f9a"), "{name}");
            }
        }
    }
}

/// `cargo test -p dusk_core --release -- --ignored --nocapture speed_on_this_repository`
/// prints how fast the Rust files of this repository parse, for the design's numbers.
#[test]
#[ignore = "a measurement, not a check"]
fn speed_on_this_repository() {
    // The first parse builds the language: its grammar and its query.
    let started = Instant::now();
    rust("").unwrap();
    println!("first use of the language: {:?}", started.elapsed());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (mut files, mut lines, mut bytes, mut symbols, mut refs) = (0, 0, 0, 0, 0);
    let mut skipped = Vec::new();
    let mut slowest = (std::time::Duration::ZERO, String::new());
    let mut total = std::time::Duration::ZERO;
    for entry in ignore::WalkBuilder::new(&root).build().flatten() {
        let Ok(relative) = entry.path().strip_prefix(&root) else {
            continue;
        };
        let path = relative.to_string_lossy().replace('\\', "/");
        let Some(language) = Language::of(&path) else {
            continue;
        };
        let Ok(source) = std::fs::read(entry.path()) else {
            continue;
        };
        let started = Instant::now();
        let parsed = parse(language, &path, &source).unwrap();
        let took = started.elapsed();
        total += took;
        files += 1;
        bytes += source.len();
        lines += source.iter().filter(|byte| **byte == b'\n').count();
        if took > slowest.0 {
            slowest = (took, format!("{path}, {} bytes", source.len()));
        }
        match parsed.outline {
            Ok(outline) => {
                symbols += outline.symbols.len();
                refs += outline.refs.len();
            }
            Err(why) => skipped.push(format!("{path}: {}", why.as_str())),
        }
    }
    println!("{files} files, {lines} lines, {bytes} bytes: {total:?} on one core");
    println!("{symbols} symbols, {refs} refs, skipped: {skipped:?}");
    println!("slowest: {:?} ({})", slowest.0, slowest.1);
    assert!(files > 0);
}
