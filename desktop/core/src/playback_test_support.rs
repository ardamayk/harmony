use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

pub(crate) struct ControlledStreamServer {
    pub(crate) base_url: String,
    is_shutdown: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
}

impl ControlledStreamServer {
    pub(crate) fn start(mp3: Vec<u8>, aac: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stream fixture server");
        listener
            .set_nonblocking(true)
            .expect("configure stream fixture server");
        let base_url = format!("http://{}", listener.local_addr().expect("fixture address"));
        let is_shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = is_shutdown.clone();
        let thread =
            thread::spawn(move || serve_controlled_streams(listener, &thread_shutdown, &mp3, &aac));
        Self {
            base_url,
            is_shutdown,
            thread,
        }
    }

    pub(crate) fn shutdown(self) {
        self.is_shutdown.store(true, Ordering::Release);
        self.thread.join().expect("stop stream fixture server");
    }
}

pub(crate) fn generate_audio_fixture(directory: &Path, name: &str, codec: &str) -> Vec<u8> {
    let path = directory.join(name);
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=stereo",
            "-t",
            "3",
            "-c:a",
            codec,
            "-y",
        ])
        .arg(&path)
        .status()
        .expect("start ffmpeg fixture generation");
    assert!(status.success(), "ffmpeg fixture generation failed");
    fs::read(path).expect("read generated audio fixture")
}

fn serve_controlled_streams(
    listener: TcpListener,
    is_shutdown: &AtomicBool,
    mp3: &[u8],
    aac: &[u8],
) {
    while !is_shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => respond_to_stream_request(stream, mp3, aac),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("stream fixture accept failed: {error}"),
        }
    }
}

fn respond_to_stream_request(mut stream: TcpStream, mp3: &[u8], aac: &[u8]) {
    let mut request = [0_u8; 4096];
    let size = stream
        .read(&mut request)
        .expect("read stream fixture request");
    let request_text = String::from_utf8_lossy(&request[..size]);
    let path = request_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let playlist = b"#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:3\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:3.0,\nsegment.aac\n#EXT-X-ENDLIST\n";
    let (content_type, extra_headers, body): (&str, &str, &[u8]) = match path {
        "/radio.mp3" => ("audio/mpeg", "icy-name: Controlled Radio\r\n", mp3),
        "/catalog.m3u8" => ("application/vnd.apple.mpegurl", "", playlist),
        "/segment.aac" => ("audio/aac", "", aac),
        _ => ("text/plain", "", b"not found"),
    };
    let status = if path == "/" {
        "404 Not Found"
    } else {
        "200 OK"
    };
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .expect("write fixture headers");
    stream.write_all(body).expect("write fixture body");
}
