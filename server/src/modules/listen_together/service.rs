//! The Room: who is in it, what it plays, and when it ends. Everything lives
//! behind one lock that is never held across an await, which is what makes
//! the Guest limit atomic. Nothing here is written to disk or logged.

use super::limits::{self, Budget, MAX_AUDIO_STREAMS, ReadBudgets, TokenBucket};
use super::room_state::{Command, RoomState};
use crate::config::{Config, DEFAULT_LISTEN_TOGETHER_ROOM_HOURS};
use crate::gojson;
use crate::modules::library::{LibraryError, LibraryService};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

/// How long things last. Tests shorten these.
#[derive(Clone, Copy, Debug)]
pub struct Timings {
    /// A Room closes itself after this long.
    pub room_lifetime: Duration,
    /// A Guest with no open event stream for this long loses their place.
    pub guest_grace: Duration,
    /// How often the Room looks for expired Guests and its own expiry.
    pub sweep_interval: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            room_lifetime: room_lifetime(DEFAULT_LISTEN_TOGETHER_ROOM_HOURS),
            guest_grace: Duration::from_secs(60),
            sweep_interval: Duration::from_secs(1),
        }
    }
}

impl Timings {
    pub fn for_config(config: &Config) -> Self {
        Self {
            room_lifetime: room_lifetime(config.listen_together_room_hours),
            ..Self::default()
        }
    }
}

fn room_lifetime(hours: u64) -> Duration {
    Duration::from_secs(hours * 60 * 60)
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RoomError {
    #[error("Listen Together is not configured")]
    Disabled,
    #[error("a Room is already open")]
    AlreadyOpen,
    #[error("no Room is open")]
    NotFound,
    #[error("the Invite Link is invalid, expired or revoked")]
    InviteInvalid,
    #[error("the Room is full")]
    Full,
    #[error("the Guest session has ended")]
    SessionEnded,
    #[error("too many requests")]
    RateLimited,
    #[error("{0}")]
    BadCommand(&'static str),
    #[error("trackIds holds a Track that is not in the library")]
    UnknownTrack,
    #[error("read Tracks: {0}")]
    Library(String),
}

/// What the event stream carries: the latest snapshot, or the Room's end.
#[derive(Clone)]
pub enum Feed {
    State(Arc<RoomState>),
    Closed,
}

/// A live Guest session as a request holds it. It does not hold the cookie's
/// value, only the hash the Room files the session under.
#[derive(Clone)]
pub struct GuestSession {
    key: String,
    revoked: watch::Receiver<bool>,
}

impl GuestSession {
    /// Turns true when the session ends, however it ended.
    pub fn revocation(&self) -> watch::Receiver<bool> {
        self.revoked.clone()
    }

    /// Finishes when the session ends.
    pub async fn revoked(mut self) {
        let _ = self.revoked.wait_for(|revoked| *revoked).await;
    }
}

/// Who sent a command, which decides their rate limit and their view.
#[derive(Clone, Copy)]
pub enum Client<'a> {
    Owner,
    Guest(&'a GuestSession),
}

/// A joined Guest: the cookie's value and the Room as a Guest sees it.
pub struct Joined {
    pub session_token: String,
    pub state: RoomState,
}

/// One event stream. A Guest's counts as their presence until it is dropped.
pub struct Subscription {
    pub events: watch::Receiver<Feed>,
    /// Added to every snapshot; only the Owner's stream has one.
    pub invite_url: Option<String>,
    _presence: Option<Presence>,
}

struct Presence {
    service: Weak<ListenTogether>,
    generation: u64,
    key: String,
}

impl Drop for Presence {
    fn drop(&mut self) {
        if let Some(service) = self.service.upgrade() {
            service.stream_ended(self.generation, &self.key);
        }
    }
}

/// A place among a Guest's audio streams, given back when dropped.
pub struct AudioStreamSlot {
    service: Weak<ListenTogether>,
    generation: u64,
    key: String,
    id: u64,
}

impl Drop for AudioStreamSlot {
    fn drop(&mut self) {
        if let Some(service) = self.service.upgrade() {
            service.audio_stream_ended(self.generation, &self.key, self.id);
        }
    }
}

/// An open audio stream. Dropping `_superseded` tells its body to stop.
struct AudioStream {
    id: u64,
    /// The request path, which names the Track.
    target: String,
    _superseded: oneshot::Sender<()>,
}

struct Guest {
    open_streams: usize,
    /// When the last stream ended; the join counts as the first such moment.
    disconnected_at: Instant,
    commands: TokenBucket,
    reads: ReadBudgets,
    /// Oldest first.
    audio_streams: Vec<AudioStream>,
    revoked: watch::Sender<bool>,
}

/// No `Debug`: this holds the Invite Link.
struct Room {
    generation: u64,
    state: RoomState,
    closes_at: Instant,
    invite_url: String,
    /// The MAC of the invite token, compared in constant time.
    invite_key: String,
    invite_tag: Vec<u8>,
    /// Guests by the hash of their session cookie.
    guests: HashMap<String, Guest>,
    owner_commands: TokenBucket,
    feed: watch::Sender<Feed>,
}

impl Room {
    fn publish(&self) {
        self.feed
            .send_replace(Feed::State(Arc::new(self.state.clone())));
    }

    fn owner_view(&self) -> RoomState {
        RoomState {
            invite_url: Some(self.invite_url.clone()),
            ..self.state.clone()
        }
    }

    fn view(&self, client: Client<'_>) -> RoomState {
        match client {
            Client::Owner => self.owner_view(),
            Client::Guest(_) => self.state.clone(),
        }
    }

    fn require(&self, client: Client<'_>) -> Result<(), RoomError> {
        match client {
            Client::Guest(session) if !self.guests.contains_key(&session.key) => {
                Err(RoomError::SessionEnded)
            }
            _ => Ok(()),
        }
    }

    fn take_command(&mut self, client: Client<'_>, now: Instant) -> Result<(), RoomError> {
        let bucket = match client {
            Client::Owner => &mut self.owner_commands,
            Client::Guest(session) => {
                &mut self
                    .guests
                    .get_mut(&session.key)
                    .ok_or(RoomError::SessionEnded)?
                    .commands
            }
        };
        if bucket.take(now) {
            Ok(())
        } else {
            Err(RoomError::RateLimited)
        }
    }

    fn invite_matches(&self, token: &str) -> bool {
        invite_mac(&self.invite_key, token)
            .verify_slice(&self.invite_tag)
            .is_ok()
    }

    /// Tells every event stream, then ends the Guest sessions.
    fn close(self) {
        // The feed first: a stream woken by its revocation reads it to tell
        // a closed Room from a session that ended alone.
        self.feed.send_replace(Feed::Closed);
        for guest in self.guests.values() {
            guest.revoked.send_replace(true);
        }
    }
}

pub struct ListenTogether {
    library: LibraryService,
    /// The start of the Invite Link, unset when not configured.
    public_origin: Option<String>,
    max_guests: usize,
    timings: Timings,
    next_generation: AtomicU64,
    next_audio_stream: AtomicU64,
    /// Outlives every Room, and is spent even when none is open.
    joins: Mutex<TokenBucket>,
    room: Mutex<Option<Room>>,
}

impl ListenTogether {
    pub fn new(config: &Config, library: LibraryService) -> Self {
        Self::with_timings(config, library, Timings::for_config(config))
    }

    pub fn with_timings(config: &Config, library: LibraryService, timings: Timings) -> Self {
        Self {
            library,
            public_origin: config
                .listen_together
                .as_ref()
                .map(|settings| settings.public_origin.clone()),
            max_guests: config.listen_together_max_guests,
            timings,
            next_generation: AtomicU64::new(1),
            next_audio_stream: AtomicU64::new(1),
            joins: Mutex::new(TokenBucket::new(limits::JOINS, Instant::now())),
            room: Mutex::new(None),
        }
    }

    /// The Room's lock, with an expired Room already closed. Every entry
    /// goes through this, so an expired Room is never used even between
    /// sweeps.
    fn lock(&self) -> MutexGuard<'_, Option<Room>> {
        let mut slot = self.room.lock().expect("Room lock");
        if slot
            .as_ref()
            .is_some_and(|room| room.closes_at <= Instant::now())
            && let Some(room) = slot.take()
        {
            room.close();
        }
        slot
    }

    pub fn open(self: &Arc<Self>) -> Result<RoomState, RoomError> {
        let public_origin = self.public_origin.as_ref().ok_or(RoomError::Disabled)?;
        let mut slot = self.lock();
        if slot.is_some() {
            return Err(RoomError::AlreadyOpen);
        }
        let now = Instant::now();
        let token = random_token();
        let invite_key = random_token();
        let invite_tag = invite_mac(&invite_key, &token)
            .finalize()
            .into_bytes()
            .to_vec();
        let lifetime = self.timings.room_lifetime;
        let state = RoomState::new(
            self.max_guests,
            gojson::rfc3339_nano(SystemTime::now() + lifetime),
            now_ms(),
        );
        let (feed, _) = watch::channel(Feed::State(Arc::new(state.clone())));
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let room = Room {
            generation,
            state,
            closes_at: now + lifetime,
            invite_url: format!("{public_origin}/join#{token}"),
            invite_key,
            invite_tag,
            guests: HashMap::new(),
            owner_commands: TokenBucket::new(limits::COMMANDS, now),
            feed,
        };
        let view = room.owner_view();
        *slot = Some(room);
        drop(slot);
        self.spawn_sweeper(generation);
        Ok(view)
    }

    /// Revokes the Invite Link and every Guest session at once.
    pub fn close(&self) -> Result<(), RoomError> {
        let room = self.lock().take().ok_or(RoomError::NotFound)?;
        room.close();
        Ok(())
    }

    /// Runs `action` on the open Room.
    fn with_room<T>(
        &self,
        action: impl FnOnce(&mut Room) -> Result<T, RoomError>,
    ) -> Result<T, RoomError> {
        action(self.lock().as_mut().ok_or(RoomError::NotFound)?)
    }

    pub fn room(&self, client: Client<'_>) -> Result<RoomState, RoomError> {
        self.with_room(|room| {
            room.require(client)?;
            Ok(room.view(client))
        })
    }

    /// One join attempt from anyone, valid or not.
    pub fn take_join(&self) -> Result<(), RoomError> {
        let mut joins = self.joins.lock().expect("join limit lock");
        if joins.take(Instant::now()) {
            Ok(())
        } else {
            Err(RoomError::RateLimited)
        }
    }

    /// Spends one of the Guest's reads.
    pub fn take_read(&self, session: &GuestSession, budget: Budget) -> Result<(), RoomError> {
        self.with_room(|room| {
            let guest = room
                .guests
                .get_mut(&session.key)
                .ok_or(RoomError::SessionEnded)?;
            if guest.reads.take(budget, Instant::now()) {
                Ok(())
            } else {
                Err(RoomError::RateLimited)
            }
        })
    }

    /// A place for an audio stream of `target`, and the signal to end it. At
    /// the limit, a new request for a Track already streaming replaces that
    /// Track's oldest stream: a seek is such a request, and a player or proxy
    /// may not have let go of the previous one yet, so a seek is never refused.
    pub fn open_audio_stream(
        self: &Arc<Self>,
        session: &GuestSession,
        target: &str,
    ) -> Result<(AudioStreamSlot, oneshot::Receiver<()>), RoomError> {
        self.with_room(|room| {
            let guest = room
                .guests
                .get_mut(&session.key)
                .ok_or(RoomError::SessionEnded)?;
            if guest.audio_streams.len() >= MAX_AUDIO_STREAMS {
                let oldest = guest
                    .audio_streams
                    .iter()
                    .position(|stream| stream.target == target)
                    .ok_or(RoomError::RateLimited)?;
                guest.audio_streams.remove(oldest);
            }
            let id = self.next_audio_stream.fetch_add(1, Ordering::Relaxed);
            let (superseded, ended) = oneshot::channel();
            guest.audio_streams.push(AudioStream {
                id,
                target: target.to_owned(),
                _superseded: superseded,
            });
            let slot = AudioStreamSlot {
                service: Arc::downgrade(self),
                generation: room.generation,
                key: session.key.clone(),
                id,
            };
            Ok((slot, ended))
        })
    }

    fn audio_stream_ended(&self, generation: u64, key: &str, id: u64) {
        let mut slot = self.lock();
        if let Some(guest) = slot
            .as_mut()
            .filter(|room| room.generation == generation)
            .and_then(|room| room.guests.get_mut(key))
        {
            guest.audio_streams.retain(|stream| stream.id != id);
        }
    }

    /// Exchanges an invite token for a Guest session. A browser that already
    /// holds a live session keeps it, so opening the link twice does not
    /// take a second place.
    pub fn join(&self, token: &str, current_session: Option<&str>) -> Result<Joined, RoomError> {
        self.with_room(|room| {
            if !room.invite_matches(token) {
                return Err(RoomError::InviteInvalid);
            }
            if let Some(current) = current_session
                && room.guests.contains_key(&session_key(current))
            {
                return Ok(Joined {
                    session_token: current.to_owned(),
                    state: room.state.clone(),
                });
            }
            if room.guests.len() >= room.state.max_guests {
                return Err(RoomError::Full);
            }
            let now = Instant::now();
            let session_token = random_token();
            room.guests.insert(
                session_key(&session_token),
                Guest {
                    open_streams: 0,
                    disconnected_at: now,
                    commands: TokenBucket::new(limits::COMMANDS, now),
                    reads: ReadBudgets::new(now),
                    audio_streams: Vec::new(),
                    revoked: watch::channel(false).0,
                },
            );
            room.state.set_guest_count(room.guests.len());
            room.publish();
            Ok(Joined {
                session_token,
                state: room.state.clone(),
            })
        })
        .map_err(|error| match error {
            RoomError::NotFound => RoomError::InviteInvalid,
            error => error,
        })
    }

    /// The live session a cookie's value stands for.
    pub fn authenticate(&self, session_token: &str) -> Option<GuestSession> {
        let key = session_key(session_token);
        let slot = self.lock();
        let guest = slot.as_ref()?.guests.get(&key)?;
        Some(GuestSession {
            key,
            revoked: guest.revoked.subscribe(),
        })
    }

    /// Frees the Guest's place at once.
    pub fn leave(&self, session: &GuestSession) -> Result<(), RoomError> {
        self.with_room(|room| {
            let guest = room
                .guests
                .remove(&session.key)
                .ok_or(RoomError::SessionEnded)?;
            guest.revoked.send_replace(true);
            room.state.set_guest_count(room.guests.len());
            room.publish();
            Ok(())
        })
    }

    pub async fn command(
        &self,
        client: Client<'_>,
        command: Command,
    ) -> Result<RoomState, RoomError> {
        // The limit comes first, so it also bounds the library reads below.
        self.with_room(|room| room.take_command(client, Instant::now()))?;
        command.validate().map_err(RoomError::BadCommand)?;
        if let Command::PlayTracks { track_ids, .. } = &command {
            self.require_tracks(track_ids).await?;
        }
        self.with_room(|room| {
            // The session or the Room may have ended during that read.
            room.require(client)?;
            if room.state.apply(command, now_ms()) {
                room.publish();
            }
            Ok(room.view(client))
        })
    }

    /// Only library Tracks enter a Room, so Radio and External Tracks cannot.
    async fn require_tracks(&self, track_ids: &[String]) -> Result<(), RoomError> {
        let unique: HashSet<&String> = track_ids.iter().collect();
        for track_id in unique {
            match self.library.get_track(track_id).await {
                Ok(_) => {}
                Err(LibraryError::NotFound) => return Err(RoomError::UnknownTrack),
                Err(error) => return Err(RoomError::Library(error.to_string())),
            }
        }
        Ok(())
    }

    pub fn subscribe(self: &Arc<Self>, client: Client<'_>) -> Result<Subscription, RoomError> {
        self.with_room(|room| {
            room.require(client)?;
            let presence = match client {
                Client::Owner => None,
                Client::Guest(session) => {
                    if let Some(guest) = room.guests.get_mut(&session.key) {
                        guest.open_streams += 1;
                    }
                    Some(Presence {
                        service: Arc::downgrade(self),
                        generation: room.generation,
                        key: session.key.clone(),
                    })
                }
            };
            Ok(Subscription {
                events: room.feed.subscribe(),
                invite_url: matches!(client, Client::Owner).then(|| room.invite_url.clone()),
                _presence: presence,
            })
        })
    }

    fn stream_ended(&self, generation: u64, key: &str) {
        let mut slot = self.lock();
        let Some(guest) = slot
            .as_mut()
            .filter(|room| room.generation == generation)
            .and_then(|room| room.guests.get_mut(key))
        else {
            return;
        };
        guest.open_streams = guest.open_streams.saturating_sub(1);
        if guest.open_streams == 0 {
            guest.disconnected_at = Instant::now();
        }
    }

    fn spawn_sweeper(self: &Arc<Self>, generation: u64) {
        let service = Arc::downgrade(self);
        let interval = self.timings.sweep_interval;
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(interval);
            loop {
                ticks.tick().await;
                let Some(service) = service.upgrade() else {
                    return;
                };
                if !service.sweep(generation) {
                    return;
                }
            }
        });
    }

    /// Closes an expired Room and removes Guests gone too long. Returns
    /// whether the Room is still there.
    fn sweep(&self, generation: u64) -> bool {
        let mut slot = self.lock();
        let Some(room) = slot.as_mut().filter(|room| room.generation == generation) else {
            return false;
        };
        let grace = self.timings.guest_grace;
        let now = Instant::now();
        let count = room.guests.len();
        room.guests.retain(|_, guest| {
            let stays = guest.open_streams > 0
                || now.saturating_duration_since(guest.disconnected_at) < grace;
            if !stays {
                guest.revoked.send_replace(true);
            }
            stays
        });
        if room.guests.len() != count {
            room.state.set_guest_count(room.guests.len());
            room.publish();
        }
        true
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// 256 random bits, URL-safe. A v4 UUID fixes six of its bits, so three of
/// them (366 random bits from the OS CSPRNG) go through SHA-256.
fn random_token() -> String {
    let mut digest = Sha256::new();
    for _ in 0..3 {
        digest.update(uuid::Uuid::new_v4().as_bytes());
    }
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

/// Sessions are filed under this hash, so a dump of the Room's memory holds
/// no usable cookie.
fn session_key(session_token: &str) -> String {
    hex::encode(Sha256::digest(session_token.as_bytes()))
}

fn invite_mac(key: &str, token: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key");
    mac.update(token.as_bytes());
    mac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_43_url_safe_characters_and_never_repeat() {
        let first = random_token();
        assert_eq!(first.len(), 43);
        assert_eq!(URL_SAFE_NO_PAD.decode(&first).unwrap().len(), 32);
        assert_ne!(first, random_token());
    }
}
