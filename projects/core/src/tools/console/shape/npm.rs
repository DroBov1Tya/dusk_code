//! `npm`, `pnpm` and `yarn`: install progress, funding and update notices, a clean audit
//! and the test runner's passes go. These lines are matched loosely, so one that names
//! trouble or a place in a file stays whatever it looks like.

use super::troubled;

const INSTALL_CHATTER: &str = "install progress and notices";
const PASSING_TESTS: &str = "passing tests";

pub(super) fn left_out(line: &str) -> Option<&'static str> {
    let text = line.trim();
    let what = if is_chatter(text) {
        INSTALL_CHATTER
    } else if is_pass(text) {
        PASSING_TESTS
    } else {
        return None;
    };
    (!troubled(line)).then_some(what)
}

fn is_chatter(text: &str) -> bool {
    text.starts_with("npm notice")
        || text.ends_with("looking for funding")
        || text == "run `npm fund` for details"
        || text == "found 0 vulnerabilities"
        || text.starts_with("Progress: resolved ")
        || is_package_count(text)
        || is_bar(text)
        || is_yarn_step(text)
}

/// pnpm's `Packages: +12 -3`.
fn is_package_count(text: &str) -> bool {
    text.strip_prefix("Packages:").is_some_and(|counts| {
        counts.split_whitespace().all(|count| {
            count
                .strip_prefix(['+', '-'])
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
    })
}

/// pnpm's `++++++++--` under the package count.
fn is_bar(text: &str) -> bool {
    text.contains('+') && text.bytes().all(|b| b == b'+' || b == b'-')
}

/// yarn's `[2/4] Fetching packages...`.
fn is_yarn_step(text: &str) -> bool {
    let Some((step, rest)) = text.strip_prefix('[').and_then(|text| text.split_once(']')) else {
        return false;
    };
    let numbered = step.split_once('/').is_some_and(|(at, of)| {
        [at, of]
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    });
    numbered && rest.ends_with("...")
}

/// jest's `PASS src/a.test.ts` and a passing test's `✓ adds (3 ms)`, as jest and vitest
/// print them.
fn is_pass(text: &str) -> bool {
    text.starts_with("PASS ") || text.starts_with("✓ ") || text.starts_with("√ ")
}
