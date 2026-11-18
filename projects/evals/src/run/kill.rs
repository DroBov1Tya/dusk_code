//! `kill_after`: where in a run the core is killed and started again, to see that work
//! survives a crash. A point is a count of tool events, a call's start or its end, of the
//! chat or any of its agents: the places where work takes effect.

use std::ops::RangeInclusive;

use anyhow::{Result, bail, ensure};
use serde::Deserialize;

/// No task makes this many tool events; a larger bound is a typo.
const MOST: u64 = 100_000;

/// The point of a kill: one count, or a range a run picks one from by its seed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Written")]
pub struct KillAfter(RangeInclusive<u64>);

/// As `run.yml` holds it: `kill_after: 12` or `kill_after: 2..=40`.
#[derive(Deserialize)]
#[serde(untagged)]
enum Written {
    Count(u64),
    Range(String),
}

impl TryFrom<Written> for KillAfter {
    type Error = anyhow::Error;

    fn try_from(written: Written) -> Result<Self> {
        match written {
            Written::Count(count) => Self::new(count, count),
            Written::Range(text) => Self::parse(&text),
        }
    }
}

impl KillAfter {
    fn new(first: u64, last: u64) -> Result<Self> {
        ensure!(first >= 1, "kill_after counts from 1");
        ensure!(first <= last, "kill_after {first}..={last} is empty");
        ensure!(last <= MOST, "kill_after is at most {MOST}");
        Ok(Self(first..=last))
    }

    /// `N` or `A..=B`.
    pub fn parse(text: &str) -> Result<Self> {
        let number = |text: &str| {
            text.trim()
                .parse::<u64>()
                .map_err(|_| anyhow::anyhow!("kill_after: `{text}` is not N or A..=B"))
        };
        match text.split_once("..=") {
            Some((first, last)) => Self::new(number(first)?, number(last)?),
            None if text.contains("..") => bail!("kill_after: write a range as A..=B"),
            None => {
                let count = number(text)?;
                Self::new(count, count)
            }
        }
    }

    /// The point of one run: the same seed, task and run always give the same one, and
    /// the runs of a task spread over the range.
    pub fn pick(&self, seed: u64, task: &str, run: usize) -> u64 {
        let (first, last) = (*self.0.start(), *self.0.end());
        let mixed = mix(seed ^ fnv1a(task.as_bytes()) ^ mix(run as u64));
        // `last - first + 1` cannot overflow: `last` is at most MOST.
        first + mixed % (last - first + 1)
    }
}

/// SplitMix64's finalizer: spreads close inputs far apart.
fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A hash that is the same on every machine and build, unlike `DefaultHasher`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_and_a_range_are_read_and_nonsense_is_refused() {
        assert_eq!(KillAfter::parse("12").unwrap(), KillAfter(12..=12));
        assert_eq!(KillAfter::parse(" 2 ..= 40 ").unwrap(), KillAfter(2..=40));
        for wrong in ["0", "0..=3", "5..=2", "2..40", "", "x", "-1", "1..=100001"] {
            assert!(KillAfter::parse(wrong).is_err(), "{wrong:?} was taken");
        }
    }

    #[test]
    fn a_point_depends_on_seed_task_and_run_only() {
        let range = KillAfter::parse("2..=40").unwrap();
        let points: Vec<u64> = (1..=20)
            .map(|run| range.pick(7, "team-chain", run))
            .collect();
        let again: Vec<u64> = (1..=20)
            .map(|run| range.pick(7, "team-chain", run))
            .collect();
        assert_eq!(points, again);
        assert!(points.iter().all(|point| (2..=40).contains(point)));
        // Twenty runs that all fell on a few points would test a few points.
        let distinct: std::collections::BTreeSet<_> = points.iter().collect();
        assert!(distinct.len() >= 10, "{points:?}");
        let other_seed: Vec<u64> = (1..=20)
            .map(|run| range.pick(8, "team-chain", run))
            .collect();
        assert_ne!(points, other_seed);
        assert_eq!(KillAfter::parse("9").unwrap().pick(123, "x", 4), 9);
    }

    /// Fixed values: a point must not change with the platform or the compiler.
    #[test]
    fn the_hash_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(mix(0), 0xe220_a839_7b1d_cdaf);
    }
}
