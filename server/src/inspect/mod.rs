//! Media Inspection through the Strict Import Profile
//! (server/internal/modules/library/*_inspector.go). A successful result
//! guarantees that the complete audio stream decoded to its end.
//!
//! FLAC, MP3, Vorbis and WAV decode in process (symphonia for the codecs);
//! Opus and M4A decode through ffmpeg, which the Music Server already needs.
//! Inspection blocks; run it on a blocking thread.

pub mod artwork;
pub mod errors;
mod flac;
mod id3;
mod m4a;
pub mod metadata;
mod mp3;
mod ogg;
mod wav;

use errors::{Code, Failure, Result, error, io_message};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub use errors::Issue;
pub use metadata::{
    ALBUM_ARTIST_FROM_ARTIST_WARNING, MediaPosition, NormalizedMetadata, ReplayGain,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumArtwork {
    pub warning: String,
    pub mime_type: String,
    pub width: i64,
    pub height: i64,
    pub data: Vec<u8>,
    pub sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TechnicalAudio {
    pub format: String,
    pub container: String,
    pub codec: String,
    pub duration_ms: i64,
    pub sample_rate_hz: i64,
    pub channel_count: i64,
    pub bit_depth: i64,
    pub bitrate_kbps: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaInspection {
    pub metadata: NormalizedMetadata,
    pub album_artwork: AlbumArtwork,
    pub audio: TechnicalAudio,
    pub file_sha256: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub decoded_samples: u64,
    pub total_samples: u64,
    pub percent: i64,
}

pub type ProgressReporter<'a> = dyn FnMut(Progress) -> Result<()> + Send + 'a;

/// Cancellation and progress for one inspection.
pub struct Context<'a> {
    is_cancelled: &'a (dyn Fn() -> bool + Sync),
    progress: Option<&'a mut ProgressReporter<'a>>,
}

impl<'a> Context<'a> {
    pub fn new(
        is_cancelled: &'a (dyn Fn() -> bool + Sync),
        progress: Option<&'a mut ProgressReporter<'a>>,
    ) -> Self {
        Self {
            is_cancelled,
            progress,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        (self.is_cancelled)()
    }

    /// `inspectionCancellationError`.
    fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(errors::cancelled())
        } else {
            Ok(())
        }
    }

    /// `reportDecodedProgress`: percent by samples, else by bytes; 100 only
    /// once complete.
    fn report(
        &mut self,
        decoded_samples: u64,
        total_samples: u64,
        encoded_bytes: i64,
        total_bytes: i64,
        is_complete: bool,
    ) -> Result<()> {
        let Some(report) = self.progress.as_mut() else {
            return Ok(());
        };
        let mut percent = if total_samples > 0 {
            (u128::from(decoded_samples) * 100 / u128::from(total_samples)) as i64
        } else if total_bytes > 0 {
            encoded_bytes * 100 / total_bytes
        } else {
            0
        };
        if is_complete {
            percent = 100;
        } else if percent >= 100 {
            percent = 99;
        }
        report(Progress {
            decoded_samples,
            total_samples,
            percent,
        })
    }
}

/// Inspects one stable local file.
pub fn inspect(path: &Path, ctx: &mut Context<'_>) -> Result<MediaInspection> {
    ctx.check()?;
    let mut file = File::open(path).map_err(|cause| {
        error(
            Code::FileRead,
            "file",
            format!("open {}: {}", path.display(), io_message(&cause)),
        )
    })?;
    let mut header = [0u8; 12];
    let read = read_up_to(&mut file, &mut header)
        .and_then(|read| file.seek(SeekFrom::Start(0)).map(|_| read))
        .map_err(|cause| {
            error(
                Code::FileRead,
                "file",
                format!("read media signature: {}", io_message(&cause)),
            )
        })?;
    if read >= 8 && &header[4..8] == b"ftyp" {
        drop(file);
        return m4a::inspect(path, ctx);
    }
    if read < header.len() {
        let cause = if read == 0 { "EOF" } else { "unexpected EOF" };
        return Err(error(
            Code::UnsupportedFormat,
            "container",
            format!("read media signature: {cause}"),
        ));
    }
    if &header[..4] == b"fLaC" {
        flac::inspect(&mut file, ctx)
    } else if &header[..4] == b"OggS" {
        ogg::inspect(&mut file, path, ctx)
    } else if &header[..3] == b"ID3" || (header[0] == 0xff && header[1] & 0xe0 == 0xe0) {
        mp3::inspect(&mut file, ctx)
    } else if &header[..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        wav::inspect(&mut file, ctx)
    } else {
        Err(error(
            Code::UnsupportedFormat,
            "container",
            "supported media signature is missing",
        ))
    }
}

fn read_up_to(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        match file.read(&mut buffer[read..])? {
            0 => break,
            n => read += n,
        }
    }
    Ok(read)
}

/// Why hashing stopped: cancelled, or an I/O error message.
enum HashError {
    Cancelled,
    Io(String),
}

/// `hashAndRewind`: the SHA-256 and size of the whole file.
fn hash_and_rewind(
    file: &mut File,
    ctx: &Context<'_>,
) -> std::result::Result<(String, i64), HashError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut size = 0i64;
    loop {
        if ctx.is_cancelled() {
            return Err(HashError::Cancelled);
        }
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buffer[..n]);
                size += n as i64;
            }
            Err(cause) => return Err(HashError::Io(format!("hash file: {}", io_message(&cause)))),
        }
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|cause| HashError::Io(format!("rewind file: {}", io_message(&cause))))?;
    Ok((hex::encode(hasher.finalize()), size))
}

/// The usual mapping of a hashing failure.
fn hash_file(file: &mut File, ctx: &Context<'_>) -> Result<(String, i64)> {
    hash_and_rewind(file, ctx).map_err(|cause| match cause {
        HashError::Cancelled => error(
            Code::ValidationCancelled,
            "validation",
            "hash file: context canceled",
        ),
        HashError::Io(message) => error(Code::FileRead, "file", message),
    })
}

/// Average bitrate in kbit/s, rounded.
fn average_bitrate_kbps(encoded_bytes: i64, duration_ms: i64) -> i64 {
    if encoded_bytes <= 0 || duration_ms <= 0 {
        return 0;
    }
    (encoded_bytes * 8 + duration_ms / 2) / duration_ms
}

fn audio_error(message: impl std::fmt::Display) -> Failure {
    error(Code::AudioDecode, "audio", message)
}

fn file_error(message: impl std::fmt::Display) -> Failure {
    error(Code::FileRead, "file", message)
}

/// A symphonia media source over a file that counts the bytes read, for
/// progress by bytes.
struct CountingFile {
    file: File,
    read: std::sync::Arc<std::sync::atomic::AtomicI64>,
}

impl Read for CountingFile {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let n = self.file.read(buffer)?;
        self.read
            .fetch_add(n as i64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

impl Seek for CountingFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

impl symphonia::core::io::MediaSource for CountingFile {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.file.metadata().ok().map(|metadata| metadata.len())
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn never() -> bool {
        false
    }

    pub fn inspect_bytes(name: &str, data: &[u8]) -> Result<MediaInspection> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, data).unwrap();
        let mut ctx = Context::new(&never, None);
        inspect(&path, &mut ctx)
    }

    pub fn fields(failure: &Failure) -> Vec<(String, String)> {
        failure
            .issues()
            .into_iter()
            .map(|issue| (issue.code.as_str().to_owned(), issue.field))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;

    #[test]
    fn rejects_unknown_and_short_files() {
        let failure = inspect_bytes("a.bin", b"hello world, not audio").unwrap_err();
        assert_eq!(
            fields(&failure),
            [("unsupported_format".into(), "container".into())]
        );
        let failure = inspect_bytes("a.bin", b"short").unwrap_err();
        assert!(failure.to_string().contains("unexpected EOF"));
    }

    #[test]
    fn progress_percent_stays_below_100_until_complete() {
        let mut seen = Vec::new();
        {
            let mut record = |progress: super::Progress| {
                seen.push(progress.percent);
                Ok(())
            };
            let mut ctx = super::Context::new(&never, Some(&mut record));
            ctx.report(10, 10, 0, 0, false).unwrap();
            ctx.report(5, 10, 0, 0, false).unwrap();
            ctx.report(0, 0, 3, 4, false).unwrap();
            ctx.report(10, 10, 0, 0, true).unwrap();
        }
        assert_eq!(seen, [99, 50, 75, 100]);
    }
}
