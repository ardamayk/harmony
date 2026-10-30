//! How much the Guest Listener takes from anyone holding an Invite Link.
//! The sizes leave normal listening untouched (ADR 0037): a Guest browses at
//! the speed of `Budget::Library`, and only a script meets the rest.

use std::time::Duration;
use tokio::time::Instant;

/// A burst of `burst` requests passes; then `per_second`.
#[derive(Clone, Copy, Debug)]
pub struct Rate {
    burst: f64,
    per_second: f64,
}

/// Room commands, from each client.
pub const COMMANDS: Rate = Rate {
    burst: 20.0,
    per_second: 20.0,
};

/// `POST join`, from everyone together: behind a proxy no client address can
/// be trusted. Eight friends arrive at once. A slow refill matters because a
/// new session starts with full budgets.
pub const JOINS: Rate = Rate {
    burst: 20.0,
    per_second: 0.2,
};

/// Lists, covers and stream requests, so a seek draws on this too. The burst
/// covers a full Room Queue's 500 Tracks read on arrival plus a long Album
/// grid scrolled to its end.
const LIBRARY: Rate = Rate {
    burst: 1000.0,
    per_second: 60.0,
};

/// The Artists page asks for 100 photos at once. Each is looked up online at
/// most once, then stored.
const PHOTOS: Rate = Rate {
    burst: 120.0,
    per_second: 10.0,
};

/// A few per page, and a Track's waveform polled every 2s while it generates.
const LOOKUPS: Rate = Rate {
    burst: 40.0,
    per_second: 2.0,
};

/// The sync layer's clock samples and an event stream's reconnects. Separate
/// from the library, so browsing cannot cost a Guest their sync.
const ROOM: Rate = COMMANDS;

/// Audio streams one Guest holds open: one Track, plus what a second tab and
/// a player's earlier range requests leave behind.
pub const MAX_AUDIO_STREAMS: usize = 4;

/// What a Guest's read draws on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Budget {
    /// Reads that end in the Music Server's own database or files.
    Library,
    /// Artist photos, which a grid asks for many at a time.
    Photos,
    /// Reads that run ffmpeg or call an outside service.
    Lookups,
    /// In-memory Room reads.
    Room,
}

pub struct TokenBucket {
    rate: Rate,
    tokens: f64,
    refilled_at: Instant,
}

impl TokenBucket {
    pub fn new(rate: Rate, now: Instant) -> Self {
        Self {
            rate,
            tokens: rate.burst,
            refilled_at: now,
        }
    }

    pub fn take(&mut self, now: Instant) -> bool {
        let elapsed: Duration = now.saturating_duration_since(self.refilled_at);
        self.tokens =
            (self.tokens + elapsed.as_secs_f64() * self.rate.per_second).min(self.rate.burst);
        self.refilled_at = now;
        let allowed = self.tokens >= 1.0;
        if allowed {
            self.tokens -= 1.0;
        }
        allowed
    }
}

/// One Guest's read budgets.
pub struct ReadBudgets {
    library: TokenBucket,
    photos: TokenBucket,
    lookups: TokenBucket,
    room: TokenBucket,
}

impl ReadBudgets {
    pub fn new(now: Instant) -> Self {
        Self {
            library: TokenBucket::new(LIBRARY, now),
            photos: TokenBucket::new(PHOTOS, now),
            lookups: TokenBucket::new(LOOKUPS, now),
            room: TokenBucket::new(ROOM, now),
        }
    }

    pub fn take(&mut self, budget: Budget, now: Instant) -> bool {
        match budget {
            Budget::Library => &mut self.library,
            Budget::Photos => &mut self.photos,
            Budget::Lookups => &mut self.lookups,
            Budget::Room => &mut self.room,
        }
        .take(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bucket_allows_a_burst_then_refills() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new(COMMANDS, start);
        for _ in 0..20 {
            assert!(bucket.take(start));
        }
        assert!(!bucket.take(start));
        assert!(bucket.take(start + Duration::from_millis(100)));
        assert!(bucket.take(start + Duration::from_millis(100)));
        assert!(!bucket.take(start + Duration::from_millis(100)));
    }

    #[test]
    fn a_full_bucket_holds_no_more_than_its_burst() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new(JOINS, start);
        let later = start + Duration::from_secs(3600);
        for _ in 0..20 {
            assert!(bucket.take(later));
        }
        assert!(!bucket.take(later));
        // One join every five seconds after that.
        assert!(!bucket.take(later + Duration::from_secs(4)));
        assert!(bucket.take(later + Duration::from_secs(5)));
    }

    #[test]
    fn work_and_outside_calls_cost_more_than_the_library() {
        const { assert!(LOOKUPS.burst < PHOTOS.burst && PHOTOS.burst < LIBRARY.burst) };
        const { assert!(LOOKUPS.per_second < PHOTOS.per_second) };
        const { assert!(PHOTOS.per_second < LIBRARY.per_second) };
    }

    #[test]
    fn a_page_of_artist_photos_and_a_waveform_poll_stay_within_budget() {
        // The Artists page's limit, and useTrackWaveform's poll.
        const { assert!(PHOTOS.burst >= 100.0) };
        const { assert!(LOOKUPS.per_second >= 0.5) };
    }

    #[test]
    fn each_budget_runs_out_alone() {
        let now = Instant::now();
        let mut budgets = ReadBudgets::new(now);
        while budgets.take(Budget::Lookups, now) {}
        assert!(budgets.take(Budget::Library, now));
        assert!(budgets.take(Budget::Photos, now));
        assert!(budgets.take(Budget::Room, now));
    }
}
