//! Failed sign-in attempts, per client address (ADR 0042). Five failures
//! are free; after that each attempt waits twice as long as the one before,
//! up to 15 minutes. There is no per-username counter: the username is not
//! secret, so one would let anyone keep the Owner out of sign-in.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::time::{Duration, Instant};

const FREE_FAILURES: u32 = 5;
const MAX_WAIT: Duration = Duration::from_secs(900);
/// A counter nobody touched for this long starts over.
const IDLE: Duration = Duration::from_secs(24 * 60 * 60);
/// So a spray of addresses cannot exhaust memory; see `OVERFLOW`.
const MAX_ENTRIES: usize = 10_000;

/// How long after its last failure the next attempt must wait.
pub fn backoff(failures: u32) -> Duration {
    match failures.checked_sub(FREE_FAILURES) {
        None => Duration::ZERO,
        Some(doublings) if doublings >= 10 => MAX_WAIT,
        Some(doublings) => Duration::from_secs(1 << doublings).min(MAX_WAIT),
    }
}

/// The counter's key: an IPv4 address, or an IPv6 address's /64 prefix,
/// since one home or hosting customer usually holds a whole /64 and could
/// otherwise take each guess from a fresh address.
fn bucket(client: IpAddr) -> IpAddr {
    match client.to_canonical() {
        IpAddr::V6(v6) => {
            let prefix = u128::from(v6) & !((1u128 << 64) - 1);
            IpAddr::V6(Ipv6Addr::from(prefix))
        }
        v4 => v4,
    }
}

/// Where new clients count once the map is full. Evicting old counters
/// instead would hand a spray of addresses fresh free attempts.
const OVERFLOW: IpAddr = IpAddr::V6(Ipv6Addr::UNSPECIFIED);

#[derive(Default)]
pub struct Attempts {
    entries: HashMap<IpAddr, Counter>,
}

impl Attempts {
    /// Starts an attempt: the time left to wait, or `None` once it may go
    /// ahead. A started attempt counts as a failure until `succeed`, so
    /// concurrent guesses cannot all slip through before any is counted.
    pub fn begin(&mut self, client: IpAddr, now: Instant) -> Option<Duration> {
        let key = self.key(client, now);
        let wait = self.wait(&key, now);
        if !wait.is_zero() {
            return Some(wait);
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
        None
    }

    /// Ends a failed attempt: its wait runs from the answer, not from when
    /// the slow password check started.
    pub fn fail(&mut self, client: IpAddr, now: Instant) {
        let key = self.key(client, now);
        if let Some(counter) = self.entries.get_mut(&key) {
            counter.last = now;
        }
    }

    pub fn succeed(&mut self, client: IpAddr) {
        self.entries.remove(&bucket(client));
    }

    fn key(&mut self, client: IpAddr, now: Instant) -> IpAddr {
        let key = bucket(client);
        if self.entries.contains_key(&key) || self.entries.len() < MAX_ENTRIES {
            return key;
        }
        self.entries
            .retain(|_, counter| now.saturating_duration_since(counter.last) < IDLE);
        if self.entries.len() < MAX_ENTRIES {
            key
        } else {
            OVERFLOW
        }
    }

    fn wait(&self, key: &IpAddr, now: Instant) -> Duration {
        self.entries
            .get(key)
            .filter(|counter| now.saturating_duration_since(counter.last) < IDLE)
            .map_or(Duration::ZERO, |counter| {
                (counter.last + backoff(counter.failures)).saturating_duration_since(now)
            })
    }
}

struct Counter {
    failures: u32,
    last: Instant,
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
    fn the_sixth_attempt_from_an_address_waits_and_a_success_clears_it() {
        let mut attempts = Attempts::default();
        let start = Instant::now();
        for _ in 0..5 {
            assert_eq!(attempts.begin(CLIENT, start), None);
        }
        assert_eq!(attempts.begin(CLIENT, start), Some(Duration::from_secs(1)));
        // Another address, such as the Owner's, is not held up.
        let other = IpAddr::from([198, 51, 100, 1]);
        assert_eq!(attempts.begin(other, start), None);
        // Once the wait is over the attempt goes ahead, and fails again.
        let later = start + Duration::from_secs(1);
        assert_eq!(attempts.begin(CLIENT, later), None);
        assert_eq!(attempts.begin(CLIENT, later), Some(Duration::from_secs(2)));
        attempts.succeed(CLIENT);
        assert_eq!(attempts.begin(CLIENT, later), None);
    }

    #[test]
    fn counters_idle_for_a_day_start_over() {
        let mut attempts = Attempts::default();
        let start = Instant::now();
        for _ in 0..20 {
            attempts.begin(CLIENT, start + MAX_WAIT * 20);
        }
        let next_day = start + MAX_WAIT * 20 + IDLE;
        for _ in 0..5 {
            assert_eq!(attempts.begin(CLIENT, next_day), None);
        }
        assert!(attempts.begin(CLIENT, next_day).is_some());
    }

    #[test]
    fn a_full_map_counts_new_clients_together_instead_of_forgetting() {
        let mut attempts = Attempts::default();
        let start = Instant::now();
        for n in 0..MAX_ENTRIES as u32 {
            attempts.begin(IpAddr::from(n.to_be_bytes()), start);
        }
        assert!(
            attempts
                .entries
                .contains_key(&IpAddr::from(0u32.to_be_bytes()))
        );
        for n in 0..5u32 {
            let fresh = IpAddr::from([10, 1, 0, n as u8]);
            assert_eq!(attempts.begin(fresh, start), None);
        }
        assert!(
            attempts
                .begin(IpAddr::from([10, 1, 0, 99]), start)
                .is_some()
        );
        assert_eq!(attempts.entries.len(), MAX_ENTRIES + 1);
    }

    #[test]
    fn ipv6_clients_count_by_their_64_prefix() {
        let mut attempts = Attempts::default();
        let start = Instant::now();
        for n in 1..=5u16 {
            let address = Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 0, 0, 0, n);
            assert_eq!(attempts.begin(IpAddr::V6(address), start), None);
        }
        let same_prefix = Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 0xffff, 1, 2, 3);
        assert!(attempts.begin(IpAddr::V6(same_prefix), start).is_some());
        let other_prefix = Ipv6Addr::new(0x2001, 0xdb8, 1, 3, 0, 0, 0, 1);
        assert_eq!(attempts.begin(IpAddr::V6(other_prefix), start), None);
        // An IPv4-mapped address counts as that IPv4 address.
        let mapped: IpAddr = "::ffff:203.0.113.7".parse().unwrap();
        assert_eq!(bucket(mapped), CLIENT);
    }

    #[test]
    fn the_wait_runs_from_the_failed_answer() {
        let mut attempts = Attempts::default();
        let start = Instant::now();
        for _ in 0..5 {
            attempts.begin(CLIENT, start);
        }
        // The fifth check took three seconds to answer.
        let answered = start + Duration::from_secs(3);
        attempts.fail(CLIENT, answered);
        assert_eq!(
            attempts.begin(CLIENT, answered),
            Some(Duration::from_secs(1))
        );
    }
}
