//! M4A (m4a_inspector.go, mp4_metadata.go): ffprobe reads the container,
//! ffmpeg extracts the cover and decodes the audio to its end. iTunes
//! freeform ARTISTS / ALBUMARTISTS credits are read from the atoms directly.

use super::errors::{Code, Failure, Result, error, io_message, optional_artwork};
use super::metadata::{self, MAX_IDENTITY_VALUE_BYTES, Tags, atoi, go_upper, tag_string};
use super::{AlbumArtwork, Context, MediaInspection, TechnicalAudio, artwork, audio_error};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const ERROR_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
const PROBE_OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;

#[derive(Default, Deserialize)]
#[serde(default)]
struct Probe {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ProbeStream {
    index: i64,
    codec_name: String,
    codec_type: String,
    sample_rate: String,
    channels: i64,
    bits_per_raw_sample: String,
    duration: String,
    disposition: HashMap<String, i64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ProbeFormat {
    format_name: String,
    tags: HashMap<String, String>,
}

pub fn inspect(path: &Path, ctx: &mut Context<'_>) -> Result<MediaInspection> {
    let file_sha256 = hash(path, ctx)?;
    let probe = probe(path, ctx)?;
    let (audio_stream, artwork_stream) = validate_streams(&probe);
    let audio_stream = audio_stream?;
    let credits = read_structured_credits(path)
        .map_err(|cause| error(Code::InvalidMetadata, "credits", cause));
    let mut normalized = normalize(&probe.format.tags, credits.as_ref().ok());
    if let Err(credits_failure) = &credits {
        let rest = normalized
            .err()
            .unwrap_or_default()
            .exclude_fields(&["ARTIST".to_owned(), "ALBUMARTIST".to_owned()]);
        normalized = Err(Failure::join([Some(credits_failure.clone()), Some(rest)]));
    }
    let artwork =
        optional_artwork(artwork_stream.and_then(|stream| extract_artwork(path, ctx, &stream)));
    let audio = inspect_audio(path, ctx, &audio_stream);
    Failure::join([
        normalized.as_ref().err().cloned(),
        artwork.as_ref().err().cloned(),
        audio.as_ref().err().cloned(),
    ])
    .into_result()?;
    Ok(MediaInspection {
        metadata: normalized?,
        album_artwork: artwork?,
        audio: audio?,
        file_sha256,
    })
}

fn hash(path: &Path, ctx: &Context<'_>) -> Result<String> {
    let mut file = File::open(path).map_err(|cause| {
        error(
            Code::FileRead,
            "file",
            format!("open {}: {}", path.display(), io_message(&cause)),
        )
    })?;
    super::hash_and_rewind(&mut file, ctx)
        .map(|(hash, _)| hash)
        .map_err(|cause| match cause {
            super::HashError::Cancelled => super::errors::cancelled(),
            super::HashError::Io(message) => error(Code::FileRead, "file", message),
        })
}

/// Bounded capture of a child's output stream.
#[derive(Default)]
struct Bounded {
    data: Vec<u8>,
    limit: usize,
    truncated: bool,
}

fn capture(
    reader: impl Read + Send + 'static,
    limit: usize,
) -> (Arc<Mutex<Bounded>>, JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(Bounded {
        limit,
        ..Bounded::default()
    }));
    let sink = buffer.clone();
    let handle = std::thread::spawn(move || {
        let mut reader = reader;
        let mut chunk = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut buffer = sink.lock().unwrap();
            let room = buffer.limit.saturating_sub(buffer.data.len());
            if n > room {
                buffer.truncated = true;
            }
            let keep = n.min(room);
            buffer.data.extend_from_slice(&chunk[..keep]);
        }
    });
    (buffer, handle)
}

fn take(buffer: &Arc<Mutex<Bounded>>) -> Bounded {
    std::mem::take(&mut *buffer.lock().unwrap())
}

enum RunError {
    Cancelled,
    Failed(String),
}

/// `commandFailure`: the action, the exit and what the tool printed.
fn command_failure(action: &str, cause: &str, stderr: &Bounded) -> String {
    let mut details = String::from_utf8_lossy(&stderr.data).trim().to_owned();
    if stderr.truncated {
        details.push_str(" [truncated]");
    }
    if details.is_empty() {
        format!("{action}: {cause}")
    } else {
        format!("{action}: {cause}: {details}")
    }
}

fn exit_message(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        None => "signal: killed".to_owned(),
    }
}

fn spawn(program: &str, args: &[&str]) -> std::io::Result<Child> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

/// Runs a tool to its end, killing it when the inspection is cancelled.
fn run(
    ctx: &Context<'_>,
    action: &str,
    program: &str,
    args: &[&str],
    stdout_limit: usize,
) -> std::result::Result<Bounded, RunError> {
    let mut child =
        spawn(program, args).map_err(|cause| RunError::Failed(format!("{action}: {cause}")))?;
    let (stdout, stdout_reader) = capture(child.stdout.take().unwrap(), stdout_limit);
    let (stderr, stderr_reader) = capture(child.stderr.take().unwrap(), ERROR_OUTPUT_LIMIT_BYTES);
    let status = loop {
        if ctx.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(cause) => return Err(RunError::Failed(format!("{action}: {cause}"))),
        }
    };
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    if !status.success() {
        return Err(RunError::Failed(command_failure(
            action,
            &exit_message(status),
            &take(&stderr),
        )));
    }
    Ok(take(&stdout))
}

fn path_arg(path: &Path) -> &str {
    path.to_str().unwrap_or_default()
}

fn probe(path: &Path, ctx: &Context<'_>) -> Result<Probe> {
    let container = |message: String| error(Code::UnsupportedFormat, "container", message);
    let output = run(
        ctx,
        "probe M4A container",
        "ffprobe",
        &[
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path_arg(path),
        ],
        PROBE_OUTPUT_LIMIT_BYTES,
    )
    .map_err(|cause| match cause {
        RunError::Cancelled => super::errors::cancelled(),
        RunError::Failed(message) => container(message),
    })?;
    if output.truncated {
        return Err(container("M4A probe output exceeds limit".to_owned()));
    }
    let probe: Probe = serde_json::from_str(&String::from_utf8_lossy(&output.data))
        .map_err(|cause| container(format!("decode M4A probe: {cause}")))?;
    if !is_m4a(&probe) {
        return Err(container("M4A brand is missing".to_owned()));
    }
    Ok(probe)
}

fn is_m4a(probe: &Probe) -> bool {
    let name = &probe.format.format_name;
    if !name.contains("mp4") && !name.contains("m4a") {
        return false;
    }
    let tag = |key: &str| probe.format.tags.get(key).map_or("", String::as_str);
    if tag("major_brand").eq_ignore_ascii_case("M4A ") {
        return true;
    }
    tag("compatible_brands")
        .as_bytes()
        .chunks_exact(4)
        .any(|brand| brand.eq_ignore_ascii_case(b"M4A "))
}

/// The one AAC or ALAC stream and the cover stream; the audio result is
/// `Err` only when the file cannot be inspected further.
fn validate_streams(probe: &Probe) -> (Result<ProbeStream>, Result<ProbeStream>) {
    let mut audio = Vec::new();
    let mut covers = Vec::new();
    for stream in &probe.streams {
        if stream.codec_type == "audio" {
            audio.push(stream.clone());
        } else if stream.codec_type == "video" && stream.disposition.get("attached_pic") == Some(&1)
        {
            covers.push(stream.clone());
        } else {
            let failure = error(
                Code::UnsupportedFormat,
                "container",
                format!("unsupported M4A stream type {:?}", stream.codec_type),
            );
            return (Err(failure.clone()), Err(failure));
        }
    }
    if audio.len() != 1 {
        let failure = error(
            Code::UnsupportedFormat,
            "codec",
            "exactly one M4A audio stream is required",
        );
        return (Err(failure.clone()), Err(failure));
    }
    let stream = audio.remove(0);
    if stream.codec_name != "aac" && stream.codec_name != "alac" {
        let failure = error(
            Code::UnsupportedFormat,
            "codec",
            format!("unsupported M4A codec {:?}", stream.codec_name),
        );
        return (Err(failure.clone()), Err(failure));
    }
    let cover = match covers.len() {
        0 => Err(error(
            Code::MissingArtwork,
            "artwork",
            "embedded front cover is required",
        )),
        1 => Ok(covers.remove(0)),
        _ => Err(error(
            Code::InvalidArtwork,
            "artwork",
            "multiple front covers are ambiguous",
        )),
    };
    (Ok(stream), cover)
}

fn metadata_key(key: &str) -> String {
    match go_upper(key.trim()).as_str() {
        "ALBUM_ARTIST" => "ALBUMARTIST".to_owned(),
        "TRACK" => "TRACKNUMBER".to_owned(),
        "DISC" => "DISCNUMBER".to_owned(),
        other => other.to_owned(),
    }
}

fn normalize(
    raw: &HashMap<String, String>,
    credits: Option<&HashMap<String, Vec<String>>>,
) -> Result<metadata::NormalizedMetadata> {
    let mut tags = Tags::new();
    let mut keys: Vec<&String> = raw.keys().collect();
    keys.sort();
    for key in keys {
        tags.insert(metadata_key(key), vec![raw[key].clone()]);
    }
    let genre = raw
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("genre"))
        .map_or("", |(_, value)| value.as_str());
    let genres = metadata::split_genre_tag_values(&[genre]);
    if !genres.is_empty() {
        tags.insert("GENRE".to_owned(), genres);
    }
    for (key, values) in credits.into_iter().flatten() {
        tags.insert(key.clone(), values.clone());
    }
    metadata::normalize(&tags, metadata::replay_gain_from_strings(raw))
}

fn extract_artwork(path: &Path, ctx: &Context<'_>, stream: &ProbeStream) -> Result<AlbumArtwork> {
    let invalid = |message: String| error(Code::InvalidArtwork, "artwork", message);
    let mime_type = match stream.codec_name.as_str() {
        "mjpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        other => {
            return Err(invalid(format!(
                "unsupported embedded artwork codec {other:?}"
            )));
        }
    };
    let map = format!("0:{}", stream.index);
    let output = run(
        ctx,
        "extract embedded artwork",
        "ffmpeg",
        &[
            "-v",
            "error",
            "-xerror",
            "-nostdin",
            "-i",
            path_arg(path),
            "-map",
            &map,
            "-frames:v",
            "1",
            "-c",
            "copy",
            "-f",
            "image2pipe",
            "pipe:1",
        ],
        artwork::MAX_ARTWORK_SIZE_BYTES + 1,
    )
    .map_err(|cause| match cause {
        RunError::Cancelled => super::errors::cancelled(),
        RunError::Failed(message) => invalid(message),
    })?;
    artwork::validate(mime_type, &output.data)
}

fn inspect_audio(
    path: &Path,
    ctx: &mut Context<'_>,
    stream: &ProbeStream,
) -> Result<TechnicalAudio> {
    let mut audio = build_audio(stream)?;
    let progress = Some((audio.sample_rate_hz as u64, audio.duration_ms));
    let microseconds = decode_to_end(
        ctx,
        path,
        &format!("0:{}", stream.index),
        "decode M4A stream",
        progress,
    )?;
    let duration_ms = (microseconds / 1000) as i64;
    if duration_ms <= 0 {
        return Err(audio_error("decoded M4A duration is not positive"));
    }
    let decoded = duration_ms as u64 * audio.sample_rate_hz as u64 / 1000;
    ctx.report(decoded, decoded, 0, 0, true)?;
    let encoded_bytes = measure_encoded_bytes(path, ctx, stream.index)?;
    audio.duration_ms = duration_ms;
    audio.bitrate_kbps = (encoded_bytes * 8 + duration_ms / 2) / duration_ms;
    if audio.bitrate_kbps <= 0 {
        return Err(audio_error("M4A average encoded bitrate is not positive"));
    }
    Ok(audio)
}

fn build_audio(stream: &ProbeStream) -> Result<TechnicalAudio> {
    let sample_rate = atoi(&stream.sample_rate).filter(|rate| *rate > 0);
    let Some(sample_rate) = sample_rate.filter(|_| stream.channels > 0) else {
        return Err(audio_error("M4A stream has invalid technical properties"));
    };
    let seconds: f64 = stream.duration.trim().parse().unwrap_or(f64::NAN);
    if seconds.is_nan() || seconds <= 0.0 {
        return Err(audio_error("M4A stream has invalid probed duration"));
    }
    let mut bit_depth = 0;
    if stream.codec_name == "alac" {
        bit_depth = atoi(&stream.bits_per_raw_sample)
            .filter(|depth| *depth > 0)
            .ok_or_else(|| audio_error("ALAC bit depth is missing"))?;
    }
    Ok(TechnicalAudio {
        format: "m4a".into(),
        container: "m4a".into(),
        codec: stream.codec_name.clone(),
        duration_ms: (seconds * 1000.0) as i64,
        sample_rate_hz: sample_rate,
        channel_count: stream.channels,
        bit_depth,
        bitrate_kbps: 0,
    })
}

/// Decodes one stream with ffmpeg to its end and returns the decoded
/// duration in microseconds. With `progress` (sample rate, expected
/// duration in ms) the decoder's progress is reported as it runs.
pub fn decode_to_end(
    ctx: &mut Context<'_>,
    path: &Path,
    map: &str,
    action: &str,
    progress: Option<(u64, i64)>,
) -> Result<u64> {
    let mut child = spawn(
        "ffmpeg",
        &[
            "-v",
            "error",
            "-xerror",
            "-nostdin",
            "-i",
            path_arg(path),
            "-map",
            map,
            "-f",
            "null",
            "-",
            "-progress",
            "pipe:1",
            "-nostats",
        ],
    )
    .map_err(|cause| audio_error(format!("{action}: {cause}")))?;
    let (stderr, stderr_reader) = capture(child.stderr.take().unwrap(), ERROR_OUTPUT_LIMIT_BYTES);
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut decoded_microseconds = 0u64;
    let mut stopped: Option<Failure> = None;
    for line in stdout.lines() {
        if ctx.is_cancelled() {
            stopped = Some(super::errors::cancelled());
            break;
        }
        let Ok(line) = line else { break };
        let Some(value) = line.strip_prefix("out_time_us=") else {
            continue;
        };
        // ffmpeg prints N/A before the first frame; only numbers count.
        let Ok(microseconds) = value.trim().parse::<u64>() else {
            continue;
        };
        decoded_microseconds = decoded_microseconds.max(microseconds);
        if let Some((sample_rate, duration_ms)) = progress {
            let decoded = decoded_microseconds * sample_rate / 1_000_000;
            let total = duration_ms.max(0) as u64 * sample_rate / 1000;
            if let Err(failure) = ctx.report(decoded, total, 0, 0, false) {
                stopped = Some(failure);
                break;
            }
        }
    }
    if stopped.is_some() || ctx.is_cancelled() {
        let _ = child.kill();
    }
    let status = child.wait();
    let _ = stderr_reader.join();
    if ctx.is_cancelled() {
        return Err(super::errors::cancelled());
    }
    if let Some(failure) = stopped {
        return Err(failure);
    }
    match status {
        Ok(status) if status.success() => Ok(decoded_microseconds),
        Ok(status) => Err(audio_error(command_failure(
            action,
            &exit_message(status),
            &take(&stderr),
        ))),
        Err(cause) => Err(audio_error(format!("{action}: {cause}"))),
    }
}

fn measure_encoded_bytes(path: &Path, ctx: &Context<'_>, index: i64) -> Result<i64> {
    let output = run(
        ctx,
        "measure M4A packets",
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "packet=stream_index,size",
            "-of",
            "csv=p=0",
            path_arg(path),
        ],
        usize::MAX,
    )
    .map_err(|cause| match cause {
        RunError::Cancelled => super::errors::cancelled(),
        RunError::Failed(message) => audio_error(message),
    })?;
    let wanted = index.to_string();
    let mut total = 0i64;
    for line in String::from_utf8_lossy(&output.data).lines() {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 2 {
            return Err(audio_error("M4A packet record is invalid"));
        }
        if fields[2..].iter().any(|field| !field.is_empty()) {
            return Err(audio_error("M4A packet record has unexpected fields"));
        }
        if fields[0] != wanted {
            continue;
        }
        match fields[1].parse::<i64>() {
            Ok(size) if size >= 0 => total += size,
            _ => return Err(audio_error("M4A packet size is invalid")),
        }
    }
    if total <= 0 {
        return Err(audio_error("M4A encoded audio size is not positive"));
    }
    Ok(total)
}

struct Atom {
    name: [u8; 4],
    data_start: u64,
    end: u64,
}

/// iTunes freeform `ARTISTS` and `ALBUMARTISTS` credits from `moov`.
fn read_structured_credits(
    path: &Path,
) -> std::result::Result<HashMap<String, Vec<String>>, String> {
    let file =
        File::open(path).map_err(|cause| format!("open MP4 metadata: {}", io_message(&cause)))?;
    let size = file
        .metadata()
        .map_err(|cause| format!("stat MP4 metadata: {}", io_message(&cause)))?
        .len();
    let mut credits = HashMap::new();
    let mut offset = 0;
    while offset < size {
        let atom = read_atom(&file, offset, size)?;
        if &atom.name == b"moov" {
            walk(&file, atom.data_start, atom.end, 1, &mut credits)?;
            return Ok(credits);
        }
        offset = atom.end;
    }
    Err("MP4 metadata atom is missing".to_owned())
}

fn walk(
    file: &File,
    start: u64,
    end: u64,
    depth: usize,
    credits: &mut HashMap<String, Vec<String>>,
) -> std::result::Result<(), String> {
    if depth > 8 {
        return Err("MP4 metadata nesting exceeds limit".to_owned());
    }
    let mut offset = start;
    while offset < end {
        let atom = read_atom(file, offset, end)?;
        if &atom.name == b"----" {
            read_freeform_credit(file, &atom, credits)?;
        }
        if matches!(&atom.name, b"moov" | b"udta" | b"meta" | b"ilst") {
            let child_start = atom.data_start + if &atom.name == b"meta" { 4 } else { 0 };
            if child_start > atom.end {
                return Err("MP4 metadata container is truncated".to_owned());
            }
            walk(file, child_start, atom.end, depth + 1, credits)?;
        }
        offset = atom.end;
    }
    Ok(())
}

fn read_atom(file: &File, offset: u64, parent_end: u64) -> std::result::Result<Atom, String> {
    if parent_end - offset < 8 {
        return Err("MP4 atom header is truncated".to_owned());
    }
    let mut header = [0u8; 16];
    file.read_exact_at(&mut header[..8], offset)
        .map_err(|cause| format!("read MP4 atom header: {}", io_message(&cause)))?;
    let mut size = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
    let mut header_size = 8;
    if size == 1 {
        if parent_end - offset < 16 {
            return Err("extended MP4 atom header is truncated".to_owned());
        }
        file.read_exact_at(&mut header[8..], offset + 8)
            .map_err(|cause| format!("read extended MP4 atom header: {}", io_message(&cause)))?;
        size = u64::from_be_bytes(header[8..16].try_into().unwrap());
        if size > parent_end - offset {
            return Err("extended MP4 atom exceeds parent".to_owned());
        }
        header_size = 16;
    } else if size == 0 {
        size = parent_end - offset;
    }
    if size < header_size || size > parent_end - offset {
        return Err("MP4 atom size is invalid".to_owned());
    }
    Ok(Atom {
        name: header[4..8].try_into().unwrap(),
        data_start: offset + header_size,
        end: offset + size,
    })
}

fn read_freeform_credit(
    file: &File,
    atom: &Atom,
    credits: &mut HashMap<String, Vec<String>>,
) -> std::result::Result<(), String> {
    let (mut mean, mut name) = (String::new(), String::new());
    let mut values = Vec::new();
    let is_credit = |mean: &str, name: &str| {
        mean == "com.apple.iTunes" && (name == "ARTISTS" || name == "ALBUMARTISTS")
    };
    let mut offset = atom.data_start;
    while offset < atom.end {
        let child = read_atom(file, offset, atom.end)?;
        match &child.name {
            b"mean" => mean = read_atom_string(file, &child)?,
            b"name" => name = read_atom_string(file, &child)?,
            b"data" if is_credit(&mean, &name) => values.push(read_atom_string(file, &child)?),
            _ => {}
        }
        offset = child.end;
    }
    if is_credit(&mean, &name) {
        credits.entry(name).or_default().extend(values);
    }
    Ok(())
}

fn read_atom_string(file: &File, atom: &Atom) -> std::result::Result<String, String> {
    let header = if &atom.name == b"data" { 8 } else { 4 };
    let size = (atom.end - atom.data_start).checked_sub(header);
    let Some(size) = size.filter(|size| *size <= MAX_IDENTITY_VALUE_BYTES as u64) else {
        return Err("MP4 metadata value size is invalid".to_owned());
    };
    let mut value = vec![0u8; size as usize];
    file.read_exact_at(&mut value, atom.data_start + header)
        .map_err(|cause| format!("read MP4 metadata value: {}", io_message(&cause)))?;
    Ok(tag_string(&value))
}
