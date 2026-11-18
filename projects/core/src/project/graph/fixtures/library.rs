//! A lending library. MARKER_IN_DOC_COMMENT is only ever written in comments.

extern crate alloc;

use std::collections::{BTreeMap, HashMap as Map};
use std::fmt::{self, Display};
use std::io::*;
use crate::store::{self, shelf::{Shelf, Slot}};
use super::clock;
use self::rules::Rule as LendingRule;

pub const MAX_LOANS: usize = 3;
static GREETING: &str = "MARKER_IN_STRING sk-live-4f9a8b7c6d5e4f3a2b1c";

/* MARKER_IN_BLOCK_COMMENT */
pub type Shelves = BTreeMap<String, Shelf>;

#[derive(Debug, Clone)]
pub struct Book {
    pub title: String,
    slot: Slot,
}

pub enum Loan {
    Out { until: clock::Day },
    Home,
}

pub union Raw {
    bits: u32,
}

pub trait Lend {
    const LIMIT: usize;
    fn lend(&mut self, book: &Book) -> Result<Loan, LendError>;
    fn can_lend(&self) -> bool {
        true
    }
}

pub struct Library<T> {
    books: Map<String, T>,
}

impl<T: Display> Library<T> {
    pub fn new() -> Self {
        Self { books: Map::new() }
    }

    pub fn find(&self, title: &str) -> Option<&T> {
        // MARKER_IN_LINE_COMMENT find_in_comment()
        let key = normalize(title);
        self.books.get(&key)
    }
}

impl Lend for Library<Book> {
    const LIMIT: usize = MAX_LOANS;

    fn lend(&mut self, book: &Book) -> Result<Loan, LendError> {
        fn stamp() -> clock::Day {
            clock::today()
        }
        let shelf = store::shelf::open::<Shelf>(r#"MARKER_IN_RAW_STRING"#)?;
        log!("lent {}", book.title);
        shelf.remove(book.slot);
        Ok(Loan::Out { until: stamp() })
    }
}

impl fmt::Display for Book {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.title)
    }
}

impl<'a> From<&'a str> for &'a Book {
    fn from(_: &'a str) -> Self {
        todo!()
    }
}

#[derive(Debug)]
pub struct LendError;

fn normalize(title: &str) -> String {
    title.trim().to_lowercase()
}

macro_rules! log {
    ($($part:tt)*) => {};
}

pub mod rules {
    pub struct Rule;

    pub fn default_rule() -> Rule {
        Rule
    }

    mod inner {
        pub const DEPTH: u8 = b'M';
    }
}

mod remote;

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Library<Book> {
        Library::new()
    }

    #[test]
    fn finds_nothing_in_an_empty_library() {
        assert!(library().find("MARKER_IN_TEST_STRING").is_none());
        assert_eq!(normalize(" A "), String::from("a"));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    /// MARKER_IN_TEST_DOC
    async fn lends_over_the_network() {
        let _ = Loan::Home;
    }
}
