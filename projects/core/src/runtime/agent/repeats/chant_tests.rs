//! Text going round is cut where it first came back; text that repeats by its nature
//! (code, tables, lists, a word said often in prose) is left alone, and no text can
//! make the reader fail.

use super::chant::Chant;

/// Feeds `text` in parts of `part` characters, as a stream does; where it was cut.
fn read(text: &str, part: usize) -> Option<usize> {
    let mut chant = Chant::new(10);
    let chars: Vec<char> = text.chars().collect();
    let mut went_round = false;
    for piece in chars.chunks(part.max(1)) {
        let piece: String = piece.iter().collect();
        if chant.push(&piece) {
            assert!(!went_round, "told once");
            went_round = true;
        }
    }
    assert_eq!(went_round, chant.cut().is_some());
    chant.cut()
}

#[test]
fn a_sentence_said_again_and_again_is_cut_after_its_first_time() {
    let intro = "Let me look at the parser once more before I change it.\n";
    let sentence = "I will now check the configuration file for the error. ";
    let text = format!("{intro}{}", sentence.repeat(30));
    for part in [1, 3, 17, 400] {
        let cut = read(&text, part).expect("a loop");
        let kept: String = text.chars().take(cut).collect();
        assert!(kept.starts_with(intro), "{kept}");
        // What it said is kept once, and not ten times.
        assert!(kept.matches("configuration").count() <= 2, "{kept}");
        assert!(kept.contains("configuration"));
    }
}

#[test]
fn a_paragraph_said_again_is_cut_too() {
    let paragraph = "The build fails because the linker cannot find the library. I \
        should add it to the search path and try again, then read the log.\n\n";
    assert!(read(&paragraph.repeat(12), 7).is_some());
}

#[test]
fn a_few_repeats_far_apart_are_prose() {
    let mut text = String::new();
    for i in 0..12 {
        text.push_str("The test passes on this platform with the default settings. ");
        text.push_str(&format!(
            "Section {i} explains how the cache warms up, which files it reads, why the \
             order of the steps matters for a cold start, what the logs show at each \
             stage and how a reader can tell a slow disk from a slow network on a busy \
             machine; it closes with the numbers from run {i}.\n"
        ));
    }
    assert_eq!(read(&text, 5), None);
}

#[test]
fn code_tables_and_lists_are_not_read() {
    let line = "    assert_eq!(parse(\"2024-01-01\"), Ok(date(2024, 1, 1)));\n";
    let fenced = format!(
        "Here are the tests:\n```rust\n{}```\nThat is all.\n",
        line.repeat(40)
    );
    assert_eq!(read(&fenced, 9), None);
    let indented = format!("Tests:\n\n{}", line.repeat(40));
    assert_eq!(read(&indented, 9), None);
    let row = "| parser | ok | 0.01 s | no warnings at all in this crate |\n";
    let table = format!(
        "| crate | result | time | notes |\n|---|---|---|---|\n{}",
        row.repeat(40)
    );
    assert_eq!(read(&table, 4), None);
    let item = "- check that the file exists and that it can be read by the user\n";
    assert_eq!(read(&item.repeat(40), 11), None);
    let numbered: String = (1..=40)
        .map(|i| format!("{i}. run the same command again and compare its output\n"))
        .collect();
    assert_eq!(read(&numbered, 6), None);
    let quoted = "> the same line quoted from the log, again and again and again\n";
    assert_eq!(read(&quoted.repeat(40), 13), None);
    let rules = format!("{}\n", "-".repeat(80)).repeat(40);
    assert_eq!(read(&rules, 50), None);
}

#[test]
fn prose_between_list_items_does_not_add_up() {
    let mut text = String::new();
    for _ in 0..30 {
        text.push_str("Next, the same step for the next file in the list below.\n");
        text.push_str("- a file\n");
    }
    assert_eq!(read(&text, 8), None);
}

#[test]
fn a_loop_after_a_code_block_is_still_cut() {
    let text = format!(
        "```\nfn main() {{}}\n```\n{}",
        "Running it again to see whether it changed anything. ".repeat(20)
    );
    assert!(read(&text, 10).is_some());
}

#[test]
fn any_text_is_read_without_failing() {
    let odd = [
        String::new(),
        "\n".repeat(5000),
        "\u{202e}\u{0}\t\r".repeat(500),
        "é".repeat(10_000),
        "12".repeat(3000),
        format!("{}x", " ".repeat(10_000)),
        "```".repeat(1000),
    ];
    for text in odd {
        let _ = read(&text, 3);
    }
    // One character said over and over is no prose either.
    assert!(read(&"a".repeat(200), 1).is_some());
}
