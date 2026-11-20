//! `pytest`: the rows of dots and the `PASSED` lines go. A row with an `F`, an `E`, an
//! `s` or an `x` in it, the failures and errors sections, warnings and the summary stay.

const PASSING_TESTS: &str = "passing tests";

pub(super) fn left_out(line: &str) -> Option<&'static str> {
    (is_row_of_dots(line) || is_passed(line)).then_some(PASSING_TESTS)
}

/// `tests/test_a.py ........   [ 27%]` or a wrapped `.......   [ 54%]`. Without the
/// file or the percentage it could be `...` in a quoted source line, which stays.
fn is_row_of_dots(line: &str) -> bool {
    let mut words = line.split_whitespace().peekable();
    let file = words.next_if(|word| word.ends_with(".py")).is_some();
    let dots = words
        .next()
        .is_some_and(|word| word.bytes().all(|b| b == b'.'));
    let rest: String = words.collect();
    let percent = is_percent(&rest);
    dots && (percent || (file && rest.is_empty()))
}

/// `tests/test_a.py::test_x PASSED [ 10%]`, also as `pytest-xdist` prints it:
/// `[gw0] [ 10%] PASSED tests/test_a.py::test_x`.
fn is_passed(line: &str) -> bool {
    let mut passed = 0;
    let mut tests = 0;
    let mut rest = String::new();
    for word in line.split_whitespace() {
        match word {
            "PASSED" => passed += 1,
            word if word.contains("::") => tests += 1,
            word if is_worker(word) => {}
            word => rest.push_str(word),
        }
    }
    passed == 1 && tests == 1 && (rest.is_empty() || is_percent(&rest))
}

/// `[ 27%]` with its spaces taken out.
fn is_percent(text: &str) -> bool {
    text.strip_prefix('[')
        .and_then(|text| text.strip_suffix("%]"))
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// `[gw3]`.
fn is_worker(word: &str) -> bool {
    word.strip_prefix("[gw")
        .and_then(|word| word.strip_suffix(']'))
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}
