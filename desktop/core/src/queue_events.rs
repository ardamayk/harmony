use crate::connection::{HttpBridge, ServerOrigin};
use crate::shared_playback::SharedPlaybackSnapshot;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::watch;

const MAX_EVENT_BUFFER_BYTES: usize = 64 * 1024;
const QUEUE_EVENT_NAME: &str = "queue-invalidated";
/// A full Shared Playback State snapshot; carries no event ID (ADR 0040).
const PLAYBACK_STATE_EVENT_NAME: &str = "playback-state";
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// What the Music Server's event stream delivered.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ServerEvent {
    Queue(QueueEvent),
    PlaybackState(SharedPlaybackSnapshot),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEvent {
    pub revision: String,
    pub sequence: String,
    pub invalidates: Vec<String>,
    #[serde(skip)]
    last_event_id: String,
}

#[derive(Default)]
struct QueueEventDecoder {
    buffer: Vec<u8>,
}

impl QueueEventDecoder {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<ServerEvent>, String> {
        if chunk.len() > MAX_EVENT_BUFFER_BYTES.saturating_sub(self.buffer.len()) {
            self.buffer.clear();
            return Err("Music Server Queue event exceeded the safety limit.".to_owned());
        }
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(frame_end) = self.buffer.windows(2).position(|value| value == b"\n\n") {
            let frame = self.buffer.drain(..frame_end + 2).collect::<Vec<_>>();
            if let Some(event) = decode_queue_event(&frame[..frame.len() - 2])? {
                events.push(event);
            }
        }
        Ok(events)
    }
}

fn decode_queue_event(frame: &[u8]) -> Result<Option<ServerEvent>, String> {
    let frame = std::str::from_utf8(frame)
        .map_err(|_| "Music Server Queue event was not valid UTF-8.".to_owned())?;
    let mut event_id = None;
    let mut event_name = None;
    let mut data = Vec::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("id: ") {
            event_id = Some(value);
        } else if let Some(value) = line.strip_prefix("event: ") {
            event_name = Some(value);
        } else if let Some(value) = line.strip_prefix("data: ") {
            data.push(value);
        }
    }
    if event_name == Some(PLAYBACK_STATE_EVENT_NAME) {
        // A bad snapshot costs one snapshot, not the Queue events sharing the
        // stream; the next frame is a full state again.
        return Ok(
            match serde_json::from_str::<SharedPlaybackSnapshot>(&data.join("\n")) {
                Ok(snapshot) => Some(ServerEvent::PlaybackState(snapshot)),
                Err(error) => {
                    eprintln!("Music Server Playback State event was invalid: {error}");
                    None
                }
            },
        );
    }
    if event_name != Some(QUEUE_EVENT_NAME) {
        return Ok(None);
    }
    let mut event = serde_json::from_str::<QueueEvent>(&data.join("\n"))
        .map_err(|_| "Music Server Queue event payload was invalid.".to_owned())?;
    if event.revision.is_empty()
        || event.sequence.parse::<u64>().is_err()
        || !event.invalidates.iter().any(|value| value == "queue")
    {
        return Err("Music Server Queue event fields were invalid.".to_owned());
    }
    event.last_event_id = event_id
        .ok_or_else(|| "Music Server Queue event ID was missing.".to_owned())?
        .to_owned();
    Ok(Some(ServerEvent::Queue(event)))
}

pub struct QueueEventService {
    reconnect: watch::Sender<u64>,
}

impl QueueEventService {
    pub fn start(
        bridge: Arc<HttpBridge>,
        origin: Arc<RwLock<Option<ServerOrigin>>>,
        on_event: impl Fn(ServerEvent) + Send + Sync + 'static,
        on_error: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        let (reconnect, reconnect_rx) = watch::channel(0);
        crate::runtime::spawn(run_queue_event_stream(
            bridge,
            origin,
            Arc::new(on_event),
            Arc::new(on_error),
            reconnect_rx,
        ));
        Self { reconnect }
    }

    pub fn reconnect(&self) {
        self.reconnect.send_modify(|generation| *generation += 1);
    }
}

async fn run_queue_event_stream(
    bridge: Arc<HttpBridge>,
    origin: Arc<RwLock<Option<ServerOrigin>>>,
    on_event: Arc<dyn Fn(ServerEvent) + Send + Sync>,
    on_error: Arc<dyn Fn(String) + Send + Sync>,
    mut reconnect: watch::Receiver<u64>,
) {
    let mut last_event_id = None;
    loop {
        let current_origin = match origin.read() {
            Ok(origin) => origin.clone(),
            Err(_) => {
                on_error("Desktop Queue connection state is unavailable.".to_owned());
                return;
            }
        };
        let Some(current_origin) = current_origin else {
            if reconnect.changed().await.is_err() {
                return;
            }
            last_event_id = None;
            continue;
        };
        if let Err(error) = bridge.test_server(&current_origin).await {
            on_error(error.message);
            if wait_to_reconnect(&mut reconnect).await {
                return;
            }
            continue;
        }
        let response = bridge
            .open_queue_event_stream(&current_origin, last_event_id.as_deref())
            .await;
        match response {
            Ok(response) => {
                if consume_queue_event_stream(
                    response,
                    &on_event,
                    &on_error,
                    &mut last_event_id,
                    &mut reconnect,
                )
                .await
                {
                    return;
                }
            }
            Err(error) => on_error(error.message),
        }
        if wait_to_reconnect(&mut reconnect).await {
            return;
        }
    }
}

async fn consume_queue_event_stream(
    response: reqwest::Response,
    on_event: &Arc<dyn Fn(ServerEvent) + Send + Sync>,
    on_error: &Arc<dyn Fn(String) + Send + Sync>,
    last_event_id: &mut Option<String>,
    reconnect: &mut watch::Receiver<u64>,
) -> bool {
    let mut decoder = QueueEventDecoder::default();
    let mut stream = response.bytes_stream();
    loop {
        tokio::select! {
            changed = reconnect.changed() => return changed.is_err(),
            chunk = stream.next() => match chunk {
                Some(Ok(chunk)) => {
                    match decoder.push(&chunk) {
                        Ok(events) => {
                            for event in events {
                                // Only Queue frames carry an ID to resume from;
                                // a playback-state frame has none and must not
                                // clear it.
                                if let ServerEvent::Queue(event) = &event {
                                    *last_event_id = Some(event.last_event_id.clone());
                                }
                                on_event(event);
                            }
                        }
                        Err(error) => {
                            on_error(error);
                            return false;
                        }
                    }
                }
                Some(Err(_)) => {
                    on_error("Music Server Queue event stream was interrupted.".to_owned());
                    return false;
                }
                None => return false,
            }
        }
    }
}

async fn wait_to_reconnect(reconnect: &mut watch::Receiver<u64>) -> bool {
    tokio::select! {
        changed = reconnect.changed() => changed.is_err(),
        () = tokio::time::sleep(RECONNECT_DELAY) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{QueueEventDecoder, QueueEventService, ServerEvent};
    use crate::connection::{HttpBridge, ServerOrigin};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, RwLock, mpsc};
    use std::thread;
    use std::time::Duration;

    const PLAYBACK_STATE_FRAME_DATA: &str = r#"{"revision":11,"queueItemId":"item-1","trackId":"track-1","playing":true,"positionMs":42000,"positionAtMs":1000,"activeDeviceId":"device-1","activeDeviceName":"Harmony on pc","remoteCommandRevision":4,"skipRequest":null,"serverTimeMs":1500}"#;

    #[test]
    fn decoder_preserves_events_split_across_http_chunks() {
        let mut decoder = QueueEventDecoder::default();

        assert!(
            decoder
                .push(b"id: 2\nevent: queue-inva")
                .expect("partial event")
                .is_empty()
        );
        let events = decoder.push(
            b"lidated\ndata: {\"revision\":\"opaque-2\",\"sequence\":\"2\",\"invalidates\":[\"queue\"]}\n\n",
        ).expect("complete event");

        assert_eq!(events.len(), 1);
        let ServerEvent::Queue(event) = &events[0] else {
            panic!("expected a Queue event");
        };
        assert_eq!(event.revision, "opaque-2");
        assert_eq!(event.sequence, "2");
    }

    #[test]
    fn decoder_decodes_playback_state_frames_beside_queue_frames() {
        let mut decoder = QueueEventDecoder::default();
        let chunk = format!(
            "id: 7\nevent: queue-invalidated\ndata: {{\"revision\":\"opaque-7\",\"sequence\":\"7\",\"invalidates\":[\"queue\"]}}\n\nevent: playback-state\ndata: {PLAYBACK_STATE_FRAME_DATA}\n\n"
        );

        let events = decoder.push(chunk.as_bytes()).expect("both frames");

        assert_eq!(events.len(), 2);
        let ServerEvent::Queue(event) = &events[0] else {
            panic!("expected a Queue event");
        };
        assert_eq!(event.revision, "opaque-7");
        let ServerEvent::PlaybackState(snapshot) = &events[1] else {
            panic!("expected a Playback State snapshot");
        };
        assert_eq!(snapshot.revision, 11);
        assert!(snapshot.playing);
        assert_eq!(snapshot.position_ms, 42_000);
        assert_eq!(snapshot.active_device_id.as_deref(), Some("device-1"));
        assert_eq!(snapshot.remote_command_revision, 4);
        assert_eq!(snapshot.skip_request, None);
    }

    #[test]
    fn a_playback_state_frame_needs_no_id_but_a_queue_frame_still_does() {
        let mut decoder = QueueEventDecoder::default();

        let events = decoder
            .push(
                format!("event: playback-state\ndata: {PLAYBACK_STATE_FRAME_DATA}\n\n").as_bytes(),
            )
            .expect("playback state frame");
        assert!(matches!(
            events.first(),
            Some(ServerEvent::PlaybackState(_))
        ));

        let error = decoder
            .push(b"event: queue-invalidated\ndata: {\"revision\":\"opaque-1\",\"sequence\":\"1\",\"invalidates\":[\"queue\"]}\n\n")
            .expect_err("a Queue frame without an ID is still invalid");
        assert!(error.contains("ID was missing"), "{error}");
    }

    #[test]
    fn a_broken_playback_state_frame_does_not_disturb_the_queue_frame_beside_it() {
        let mut decoder = QueueEventDecoder::default();

        let events = decoder
            .push(b"event: playback-state\ndata: {\"revision\":\"x\"}\n\nid: 8\nevent: queue-invalidated\ndata: {\"revision\":\"opaque-8\",\"sequence\":\"8\",\"invalidates\":[\"queue\"]}\n\n")
            .expect("the Queue frame survives");

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ServerEvent::Queue(_)));
    }

    #[tokio::test]
    async fn reconnects_with_last_event_id_and_emits_latest_revision() {
        let (origin, requests) = serve_reconnecting_queue_events();
        let origin = ServerOrigin::parse(&origin).expect("test origin");
        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
        let service = QueueEventService::start(
            Arc::new(HttpBridge::new().expect("HTTP bridge")),
            Arc::new(RwLock::new(Some(origin))),
            move |event| {
                let _ = events_tx.send(event);
            },
            |_| {},
        );

        let first = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .expect("first event timeout")
            .expect("first event");
        let second = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .expect("second event timeout")
            .expect("second event");

        let [first, second] = [first, second].map(|event| match event {
            ServerEvent::Queue(event) => event,
            ServerEvent::PlaybackState(_) => panic!("expected only Queue events"),
        });
        assert_eq!(first.revision, "opaque-1");
        assert_eq!(second.revision, "opaque-3");
        let second_request = requests
            .recv_timeout(Duration::from_secs(3))
            .expect("first request");
        assert!(
            !second_request
                .to_ascii_lowercase()
                .contains("last-event-id")
        );
        let second_request = requests
            .recv_timeout(Duration::from_secs(3))
            .expect("second request");
        assert!(
            second_request
                .to_ascii_lowercase()
                .contains("last-event-id: 1")
        );
        drop(service);
    }

    #[tokio::test]
    async fn refuses_queue_stream_when_server_capability_is_absent() {
        let (origin, requests) = serve_queue_capability_check(&["api.v1"]);
        let origin = ServerOrigin::parse(&origin).expect("test origin");
        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
        let (errors_tx, mut errors_rx) = tokio::sync::mpsc::unbounded_channel();
        let service = QueueEventService::start(
            Arc::new(HttpBridge::new().expect("HTTP bridge")),
            Arc::new(RwLock::new(Some(origin))),
            move |event| {
                let _ = events_tx.send(event);
            },
            move |error| {
                let _ = errors_tx.send(error);
            },
        );

        let error = tokio::time::timeout(Duration::from_secs(3), errors_rx.recv())
            .await
            .expect("capability error timeout")
            .expect("capability error");
        assert!(error.contains("playback.queue-events.v1"));
        assert!(
            requests
                .recv_timeout(Duration::from_secs(3))
                .expect("capability request")
                .starts_with("GET /api/v1/health ")
        );
        assert!(events_rx.try_recv().is_err());
        drop(service);
    }

    #[tokio::test]
    async fn playback_state_frames_share_the_stream_without_touching_the_queue_resume_id() {
        let (origin, requests) = serve_mixed_event_stream();
        let origin = ServerOrigin::parse(&origin).expect("test origin");
        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
        let service = QueueEventService::start(
            Arc::new(HttpBridge::new().expect("HTTP bridge")),
            Arc::new(RwLock::new(Some(origin))),
            move |event| {
                let _ = events_tx.send(event);
            },
            |_| {},
        );

        // Queue frame, then a playback-state frame without an ID, then the
        // server closes; the client resumes after the Queue frame.
        let first = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .expect("Queue event timeout")
            .expect("Queue event");
        let second = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .expect("playback state timeout")
            .expect("playback state");
        assert!(matches!(first, ServerEvent::Queue(_)));
        let ServerEvent::PlaybackState(snapshot) = second else {
            panic!("expected a Playback State snapshot");
        };
        assert_eq!(snapshot.revision, 11);

        let _ = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .expect("resumed event timeout")
            .expect("resumed event");
        requests
            .recv_timeout(Duration::from_secs(3))
            .expect("first request");
        let reconnect_request = requests
            .recv_timeout(Duration::from_secs(3))
            .expect("reconnect request");
        assert!(
            reconnect_request
                .to_ascii_lowercase()
                .contains("last-event-id: 7"),
            "the playback-state frame must not clear the Queue resume ID: {reconnect_request}"
        );
        drop(service);
    }

    /// The next API request. Whatever probes local ports on a developer's
    /// machine reaches these servers too, and must not take a request's place.
    fn accept_api(listener: &TcpListener) -> TcpStream {
        loop {
            let (stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .expect("set read timeout");
            let mut start = [0_u8; 9];
            if stream
                .peek(&mut start)
                .is_ok_and(|_| start == *b"GET /api/")
            {
                stream.set_read_timeout(None).expect("clear read timeout");
                return stream;
            }
        }
    }

    fn serve_mixed_event_stream() -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mixed SSE server");
        let address = listener.local_addr().expect("mixed SSE address");
        let (requests_tx, requests_rx) = mpsc::channel();
        thread::spawn(move || {
            for connection in 0..2 {
                let mut health_stream = accept_api(&listener);
                let mut health_request = [0_u8; 4096];
                let _ = health_stream
                    .read(&mut health_request)
                    .expect("read health request");
                let health = serde_json::json!({
                    "status": "ok",
                    "version": "0.1.0-test",
                    "capabilities": [
                        "api.v1",
                        "playback.queue-events.v1",
                        "managed-import-batches.v1",
                        "managed-track-deletion.v1",
                        "managed-track-replacement.v1"
                    ],
                })
                .to_string();
                write!(
                    health_stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{health}",
                    health.len()
                )
                .expect("write health response");

                let mut stream = accept_api(&listener);
                let mut request = [0_u8; 4096];
                let size = stream.read(&mut request).expect("read Queue SSE request");
                requests_tx
                    .send(String::from_utf8_lossy(&request[..size]).into_owned())
                    .expect("record Queue SSE request");
                let body = if connection == 0 {
                    format!(
                        "id: 7\nevent: queue-invalidated\ndata: {{\"revision\":\"opaque-7\",\"sequence\":\"7\",\"invalidates\":[\"queue\"]}}\n\nevent: playback-state\ndata: {PLAYBACK_STATE_FRAME_DATA}\n\n"
                    )
                } else {
                    "id: 8\nevent: queue-invalidated\ndata: {\"revision\":\"opaque-8\",\"sequence\":\"8\",\"invalidates\":[\"queue\"]}\n\n"
                    .to_owned()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("write Queue SSE response");
            }
        });
        (format!("http://{address}"), requests_rx)
    }

    fn serve_reconnecting_queue_events() -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind Queue SSE server");
        let address = listener.local_addr().expect("Queue SSE address");
        let (requests_tx, requests_rx) = mpsc::channel();
        thread::spawn(move || {
            for (sequence, revision) in [("1", "opaque-1"), ("3", "opaque-3")] {
                let mut health_stream = accept_api(&listener);
                let mut health_request = [0_u8; 4096];
                let _ = health_stream
                    .read(&mut health_request)
                    .expect("read health request");
                let health = serde_json::json!({
                    "status": "ok",
                    "version": "0.1.0-test",
                    "capabilities": [
                        "api.v1",
                        "playback.queue-events.v1",
                        "managed-import-batches.v1",
                        "managed-track-deletion.v1",
                        "managed-track-replacement.v1"
                    ],
                })
                .to_string();
                write!(
                    health_stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{health}",
                    health.len()
                )
                .expect("write health response");

                let mut stream = accept_api(&listener);
                let mut request = [0_u8; 4096];
                let size = stream.read(&mut request).expect("read Queue SSE request");
                requests_tx
                    .send(String::from_utf8_lossy(&request[..size]).into_owned())
                    .expect("record Queue SSE request");
                let event = format!(
                    "id: {sequence}\nevent: queue-invalidated\ndata: {{\"revision\":\"{revision}\",\"sequence\":\"{sequence}\",\"invalidates\":[\"queue\"]}}\n\n"
                );
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{event}",
                    event.len()
                )
                .expect("write Queue SSE response");
            }
        });
        (format!("http://{address}"), requests_rx)
    }

    fn serve_queue_capability_check(capabilities: &[&str]) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind capability server");
        let address = listener.local_addr().expect("capability server address");
        let body = serde_json::json!({
            "status": "ok",
            "version": "0.1.0-test",
            "capabilities": capabilities,
        })
        .to_string();
        let (requests_tx, requests_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut stream = accept_api(&listener);
            let mut request = [0_u8; 4096];
            let size = stream.read(&mut request).expect("read capability request");
            requests_tx
                .send(String::from_utf8_lossy(&request[..size]).into_owned())
                .expect("record capability request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write capability response");
        });
        (format!("http://{address}"), requests_rx)
    }
}
