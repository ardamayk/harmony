//! How each audio stream through the media proxy went: when the Music Server
//! answered, what the stream waited on and how it ended. The time splits into
//! waiting for the Music Server's next chunk and waiting for mpv to take the
//! last one, which tells a stalled server or network from a player that
//! stopped reading. One stderr line per stream, naming only its source type
//! and range, never its URL; with Sentry configured (ADR 0033) also a
//! transaction in the trace of the load it opened for.

use crate::observability::{self, LoadTrace};
use axum::body::Bytes;
use futures_util::{Stream, StreamExt};
use sentry::protocol::SpanId;
use std::time::{Duration, Instant};

/// How a stream ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamOutcome {
    /// The whole answer reached mpv.
    Finished,
    /// mpv left before the end: a seek, the next file or a closed player.
    PlayerClosed,
    /// The Music Server could not be reached or broke off.
    Failed,
}

impl StreamOutcome {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::PlayerClosed => "player_closed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Wait {
    pub(crate) total: Duration,
    pub(crate) longest: Duration,
}

impl Wait {
    fn add(&mut self, waited: Duration) {
        self.total += waited;
        self.longest = self.longest.max(waited);
    }
}

/// One finished stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StreamReport {
    pub(crate) source_type: &'static str,
    pub(crate) range: String,
    pub(crate) status: Option<u16>,
    pub(crate) outcome: StreamOutcome,
    /// From mpv's request to the Music Server's headers, then its first byte.
    pub(crate) headers: Option<Duration>,
    pub(crate) first_byte: Option<Duration>,
    pub(crate) elapsed: Duration,
    pub(crate) bytes: u64,
    /// `Content-Length`: hyper stops at that many bytes without asking for
    /// the end, so reaching it is the end.
    pub(crate) length: Option<u64>,
    pub(crate) server: Wait,
    pub(crate) player: Wait,
    pub(crate) load: LoadTrace,
    pub(crate) span_id: SpanId,
}

impl StreamReport {
    /// The stderr line.
    pub(crate) fn line(&self) -> String {
        let milliseconds = |duration: Option<Duration>| {
            duration.map_or("none".to_owned(), |duration| {
                duration.as_millis().to_string()
            })
        };
        format!(
            "Media stream ended: type={} range={} status={} outcome={} headers_ms={} first_byte_ms={} open_ms={} bytes={} server_wait_ms={} server_wait_longest_ms={} player_wait_ms={} player_wait_longest_ms={}",
            self.source_type,
            self.range,
            self.status
                .map_or("none".to_owned(), |status| status.to_string()),
            self.outcome.name(),
            milliseconds(self.headers),
            milliseconds(self.first_byte),
            self.elapsed.as_millis(),
            self.bytes,
            self.server.total.as_millis(),
            self.server.longest.as_millis(),
            self.player.total.as_millis(),
            self.player.longest.as_millis(),
        )
    }
}

/// Watches one stream and reports it when dropped, however it ended.
pub(crate) struct StreamTimer {
    report: StreamReport,
    started: Instant,
    since: Instant,
    /// Whose turn it is: the Music Server's next chunk, or mpv taking the last.
    at_server: bool,
}

impl StreamTimer {
    pub(crate) fn start(source_type: &'static str, range: Option<&str>) -> Self {
        Self::for_load(source_type, range, observability::load_trace())
    }

    fn for_load(source_type: &'static str, range: Option<&str>, load: LoadTrace) -> Self {
        let now = Instant::now();
        Self {
            report: StreamReport {
                source_type,
                range: range_label(range),
                status: None,
                outcome: StreamOutcome::PlayerClosed,
                headers: None,
                first_byte: None,
                elapsed: Duration::ZERO,
                bytes: 0,
                length: None,
                server: Wait::default(),
                player: Wait::default(),
                load,
                span_id: SpanId::default(),
            },
            started: now,
            since: now,
            at_server: true,
        }
    }

    /// What the upstream request carries so the Music Server joins this trace.
    pub(crate) fn trace_header(&self) -> Option<String> {
        observability::stream_trace_header(&self.report)
    }

    pub(crate) fn answered(&mut self, status: u16, length: Option<u64>) {
        self.report.status = Some(status);
        self.report.length = length;
        self.report.headers = Some(self.started.elapsed());
    }

    pub(crate) fn failed(&mut self) {
        self.report.outcome = StreamOutcome::Failed;
    }

    /// The answer was a playlist, read here rather than streamed to mpv.
    pub(crate) fn finished(&mut self) {
        self.report.outcome = StreamOutcome::Finished;
    }

    fn turn(&mut self) {
        let waited = self.since.elapsed();
        self.since = Instant::now();
        if self.at_server {
            self.report.server.add(waited);
        } else {
            self.report.player.add(waited);
        }
        self.at_server = !self.at_server;
    }

    fn chunk<E>(&mut self, chunk: Option<&Result<Bytes, E>>) {
        match chunk {
            Some(Ok(chunk)) => {
                self.report.bytes += chunk.len() as u64;
                self.report.first_byte.get_or_insert(self.started.elapsed());
                if self.report.length == Some(self.report.bytes) {
                    self.finished();
                }
            }
            Some(Err(_)) => self.failed(),
            None => self.finished(),
        }
    }

    /// `upstream`, timed until it ends or the player drops it.
    pub(crate) fn watch<S, E>(self, upstream: S) -> impl Stream<Item = Result<Bytes, E>>
    where
        S: Stream<Item = Result<Bytes, E>> + Unpin,
    {
        // The wait for the headers was the Music Server's turn.
        let mut timer = self;
        timer.turn();
        futures_util::stream::unfold((upstream, timer), |(mut upstream, mut timer)| async move {
            timer.turn();
            let chunk = upstream.next().await;
            timer.turn();
            timer.chunk(chunk.as_ref());
            chunk.map(|chunk| (chunk, (upstream, timer)))
        })
        // `unfold` panics if polled again after its end.
        .fuse()
    }
}

impl Drop for StreamTimer {
    fn drop(&mut self) {
        self.turn();
        self.report.elapsed = self.started.elapsed();
        eprintln!("{}", self.report.line());
        observability::media_stream(&self.report);
    }
}

/// The request's `Range` as `<first>-[<last>]`, or `none`, or `invalid`.
fn range_label(range: Option<&str>) -> String {
    let Some(range) = range else {
        return "none".to_owned();
    };
    let spec = range.trim();
    let spec = spec.strip_prefix("bytes=").unwrap_or(spec);
    if !spec.is_empty()
        && spec.len() <= 64
        && spec
            .chars()
            .all(|character| character.is_ascii_digit() || character == '-')
    {
        spec.to_owned()
    } else {
        "invalid".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use std::convert::Infallible;

    fn timer() -> StreamTimer {
        StreamTimer::for_load("track", Some("bytes=4-"), LoadTrace::default())
    }

    #[test]
    fn labels_a_range_without_repeating_what_it_cannot_read() {
        assert_eq!(range_label(None), "none");
        assert_eq!(range_label(Some("bytes=0-")), "0-");
        assert_eq!(range_label(Some("bytes=4-7")), "4-7");
        assert_eq!(range_label(Some("bytes=https://a.example")), "invalid");
    }

    #[test]
    fn the_line_names_the_stream_and_never_its_url() {
        let mut timer = timer();
        timer.answered(206, None);
        let line = timer.report.line();
        assert!(
            line.starts_with(
                "Media stream ended: type=track range=4- status=206 outcome=player_closed headers_ms="
            ),
            "{line}"
        );
        assert!(line.contains(" first_byte_ms=none "), "{line}");
    }

    #[tokio::test]
    async fn a_stream_stays_ended_past_its_end() {
        let chunks = [Ok::<_, Infallible>(Bytes::from_static(b"ab"))];
        let mut watched = Box::pin(timer().watch(stream::iter(chunks)));
        assert_eq!(watched.next().await.map(|chunk| chunk.is_ok()), Some(true));
        assert!(watched.next().await.is_none());
        assert!(watched.next().await.is_none());
    }

    #[test]
    fn time_goes_to_whoever_was_awaited() {
        let mut timer = timer();
        std::thread::sleep(Duration::from_millis(20));
        timer.turn(); // The chunk arrived; mpv's turn.
        std::thread::sleep(Duration::from_millis(40));
        timer.turn();
        let (server, player) = (timer.report.server, timer.report.player);
        assert!(server.total >= Duration::from_millis(20), "{server:?}");
        assert!(player.total >= Duration::from_millis(40), "{player:?}");
        assert_eq!(player.longest, player.total);
    }

    /// hyper stops at `Content-Length` and drops the stream unasked.
    #[test]
    fn a_stream_that_reached_its_length_is_finished_though_never_asked_for_its_end() {
        let mut timer = timer();
        timer.answered(200, Some(4));
        timer.chunk(Some(&Ok::<_, ()>(Bytes::from_static(b"ab"))));
        assert_eq!(timer.report.outcome, StreamOutcome::PlayerClosed);
        timer.chunk(Some(&Ok::<_, ()>(Bytes::from_static(b"cd"))));
        assert_eq!(timer.report.outcome, StreamOutcome::Finished);
    }

    #[test]
    fn the_outcome_stays_player_closed_until_the_stream_ends() {
        let mut timer = timer();
        timer.chunk(Some(&Ok::<_, ()>(Bytes::from_static(b"ab"))));
        assert_eq!(timer.report.outcome, StreamOutcome::PlayerClosed);
        assert_eq!(timer.report.bytes, 2);
        assert!(timer.report.first_byte.is_some());
        timer.chunk::<()>(None);
        assert_eq!(timer.report.outcome, StreamOutcome::Finished);
        timer.chunk(Some(&Err::<Bytes, _>(())));
        assert_eq!(timer.report.outcome, StreamOutcome::Failed);
    }
}
