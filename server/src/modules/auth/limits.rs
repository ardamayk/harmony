//! Failed sign-in attempts, per client address and per username (ADR 0042).
//! Five failures are free; after that each attempt waits twice as long as
//! the one before, up to 15 minutes. Nothing locks an account: the wait
//! ends, and a success clears it.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::time::{Duration, Instant};

const FREE_FAILURES: u32 = 5;
const MAX_WAIT: Duration = Duration::from_secs(900);
/// A counter nobody touched for this long starts over.
const IDLE: Duration = Duration::from_secs(24 * 60 * 60);
/// Per map, so a spray of addresses or usernames cannot exhaust memory.
const MAX_ENTRIES: usize = 10_000;
/// Longer usernames cannot match one (config allows 256 characters), so
/// they share a key instead of growing the map.
const MAX_USERNAME_KEY_BYTES: usize = 1024;

/// How long after its last failure the next attempt must wait.
pub fn backoff(failures: u32) -> Duration {
    match failures.checked_sub(FREE_FAILURES) {
        None => Duration::ZERO,
        Some(doublings) if doublings >= 10 => MAX_WAIT,
        Some(doublings) => Duration::from_secs(1 << doublings).min(MAX_WAIT),
    }
}

#[derive(Default)]
pub struct Attempts {
    by_client: Counters<IpAddr>,
    by_username: Counters<String>,
}

impl Attempts {
    /// Starts an attempt: the time left to wait, or `None` once it may go
    /// ahead. A started attempt counts as a failure until `succeed`, so
    /// concurrent guesses cannot all slip through before any is counted.
    pub fn begin(&mut self, client: IpAddr, username: &str, now: Instant) -> Option<Duration> {
        let username = username_key(username);
        let wait = self
            .by_client
            .wait(&client, now)
            .max(self.by_username.wait(&username, now));
        if !wait.is_zero() {
            return Some(wait);
        }
        self.by_client.fail(client, now);
        self.by_username.fail(username, now);
        None
    }

    pub fn succeed(&mut self, client: IpAddr, username: &str) {
        self.by_client.entries.remove(&client);
        self.by_username.entries.remove(&username_key(username));
    }
}

fn username_key(username: &str) -> String {
    let mut end = username.len().min(MAX_USERNAME_KEY_BYTES);
    while !username.is_char_boundary(end) {
        end -= 1;
    }
    username[..end].to_owned()
}

struct Counter {
    failures: u32,
    last: Instant,
}

struct Counters<K> {
    entries: HashMap<K, Counter>,
}

impl<K> Default for Counters<K> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> Counters<K> {
    fn wait(&self, key: &K, now: Instant) -> Duration {
        self.entries
            .get(key)
            .filter(|counter| now.saturating_duration_since(counter.last) < IDLE)
            .map_or(Duration::ZERO, |counter| {
                (counter.last + backoff(counter.failures)).saturating_duration_since(now)
            })
    }

    fn fail(&mut self, key: K, now: Instant) {
        if !self.entries.contains_key(&key) && self.entries.len() >= MAX_ENTRIES {
            self.entries
                .retain(|_, counter| now.saturating_duration_since(counter.last) < IDLE);
            if self.entries.len() >= MAX_ENTRIES
                && let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, counter)| counter.last)
                    .map(|(key, _)| key.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        let counter = self.entries.entry(key).or_insert(Counter {
            failures: 0,
            last: now,
        });
        if now.saturating_duration_since(counter.last) >= IDLE {
            counter.failures = 0;
        }
        counter.failures = counter.failures.saturating_add(1);
        counter.last = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 7));

    #[test]
    fn five_failures_are_free_then_each_wait_doubles_up_to_fifteen_minutes() {
        let waits: Vec<u64> = (0..=16)
            .map(|failures| backoff(failures).as_secs())
            .collect();
        assert_eq!(
            waits,
            [
                0, 0, 0, 0, 0, 1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 900, 900
            ]
        );
        assert_eq!(backoff(u32::MAX), MAX_WAIT);
    }

    #[test]
    fn the_sixth_attempt_waits_and_a_success_clears_the_counters() {
        let mut attempts = Attempts::default();
        let start = Instant::now();
        for _ in 0..5 {
            assert_eq!(attempts.begin(CLIENT, "owner", start), None);
        }
        assert_eq!(
            attempts.begin(CLIENT, "owner", start),
            Some(Duration::from_secs(1))
        );
        // Another address still meets the username's wait.
        let other = IpAddr::from([198, 51, 100, 1]);
        assert!(attempts.begin(other, "owner", start).is_some());
        // Once the wait is over the attempt goes ahead, and fails again.
        let later = start + Duration::from_secs(1);
        assert_eq!(attempts.begin(CLIENT, "owner", later), None);
        assert_eq!(
            attempts.begin(CLIENT, "owner", later),
            Some(Duration::from_secs(2))
        );
        attempts.succeed(CLIENT, "owner");
        assert_eq!(attempts.begin(CLIENT, "owner", later), None);
    }

    #[test]
    fn counters_idle_for_a_day_start_over() {
        let mut attempts = Attempts::default();
        let start = Instant::now();
        for _ in 0..20 {
            attempts.begin(CLIENT, "owner", start + MAX_WAIT * 20);
        }
        let next_day = start + MAX_WAIT * 20 + IDLE;
        for _ in 0..5 {
            assert_eq!(attempts.begin(CLIENT, "owner", next_day), None);
        }
        assert!(attempts.begin(CLIENT, "owner", next_day).is_some());
    }

    #[test]
    fn a_full_map_forgets_its_oldest_entry() {
        let mut counters = Counters::<u32>::default();
        let start = Instant::now();
        for key in 0..MAX_ENTRIES as u32 {
            counters.fail(key, start + Duration::from_millis(u64::from(key)));
        }
        counters.fail(u32::MAX, start + Duration::from_secs(60));
        assert_eq!(counters.entries.len(), MAX_ENTRIES);
        assert!(!counters.entries.contains_key(&0));
        assert!(counters.entries.contains_key(&u32::MAX));
    }
}
