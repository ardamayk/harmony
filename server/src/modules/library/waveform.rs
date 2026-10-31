//! Lazily generated waveform peaks (library/waveform.go): ffmpeg decodes a
//! Track to mono 8 kHz PCM, the samples fold into 400 peaks normalized to
//! 0..255, and the result is cached against the file's size and mtime.

use super::repository::{LibraryError, Repository, WaveformRecord};
use super::types::TrackWaveform;
use crate::observability;
use futures_util::FutureExt;
use futures_util::future::{BoxFuture, Shared};
use sentry::SentryFutureExt;
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::Semaphore;

/// How many bars the Player Bar draws behind the seek bar.
pub const PEAK_COUNT: usize = 400;
const SAMPLE_RATE_HZ: i64 = 8000;
const GENERATION_TIMEOUT: Duration = Duration::from_secs(60);
/// Generation is CPU-bound; two at once keeps a burst from starving imports
/// and playback.
const CONCURRENCY: usize = 2;
/// A request waits this long for a fresh waveform before answering 202.
const PENDING_AFTER: Duration = Duration::from_secs(2);
const ERROR_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum WaveformError {
    #[error("waveform generation is unavailable")]
    Unavailable,
    #[error("waveform generation is pending")]
    Pending,
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error("{0}")]
    Generation(String),
}

/// Folds samples into a fixed number of bins, keeping the loudest absolute
/// sample per bin. With an unknown total every sample is kept and bucketed
/// at the end.
pub struct PeakBucketer {
    count: usize,
    total_samples: i64,
    seen: i64,
    peaks: Vec<i32>,
    buffered: Vec<i16>,
}

impl PeakBucketer {
    pub fn new(count: usize, total_samples: i64) -> Self {
        Self {
            count,
            total_samples,
            seen: 0,
            peaks: vec![0; count],
            buffered: Vec::new(),
        }
    }

    pub fn add(&mut self, samples: &[i16]) {
        if self.total_samples <= 0 {
            self.buffered.extend_from_slice(samples);
            return;
        }
        for &sample in samples {
            let bin =
                ((self.seen * self.count as i64) / self.total_samples).min(self.count as i64 - 1);
            let magnitude = i32::from(sample).abs();
            let peak = &mut self.peaks[bin as usize];
            if magnitude > *peak {
                *peak = magnitude;
            }
            self.seen += 1;
        }
    }

    /// Normalizes to 0..255 against the loudest bin; silence is all zeros.
    pub fn finish(mut self) -> Vec<u8> {
        if self.total_samples <= 0 && !self.buffered.is_empty() {
            self.total_samples = self.buffered.len() as i64;
            let buffered = std::mem::take(&mut self.buffered);
            self.add(&buffered);
        }
        let loudest = self.peaks.iter().copied().max().unwrap_or(0);
        if loudest == 0 {
            return vec![0; self.count];
        }
        self.peaks
            .iter()
            .map(|&peak| (i64::from(peak) * 255 / i64::from(loudest)) as u8)
            .collect()
    }
}

/// Raw signed 16-bit little-endian PCM in, normalized peaks out.
#[cfg(test)]
pub fn bucket_peaks(pcm: &[u8], count: usize, total_samples: i64) -> Vec<u8> {
    let mut bucketer = PeakBucketer::new(count, total_samples);
    bucketer.add(&decode_pcm16(pcm));
    bucketer.finish()
}

fn decode_pcm16(pcm: &[u8]) -> Vec<i16> {
    pcm.as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| i16::from_le_bytes(pair))
        .collect()
}

async fn read_pcm_stream(
    mut reader: impl AsyncRead + Unpin,
    bucketer: &mut PeakBucketer,
) -> std::io::Result<()> {
    let mut chunk = vec![0_u8; 16384];
    let mut carry: Option<u8> = None;
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        let mut data = Vec::with_capacity(read + 1);
        data.extend(carry.take());
        data.extend_from_slice(&chunk[..read]);
        if data.len() % 2 == 1 {
            carry = data.pop();
        }
        bucketer.add(&decode_pcm16(&data));
    }
}

/// Decodes with ffmpeg and buckets on the fly, so memory stays flat however
/// long the Track.
async fn generate_with_ffmpeg(path: String, duration_ms: i64) -> Result<Vec<u8>, String> {
    let mut command = tokio::process::Command::new("ffmpeg");
    command
        .args([
            "-v", "error", "-nostdin", "-i", &path, "-vn", "-ac", "1", "-ar",
        ])
        .arg(SAMPLE_RATE_HZ.to_string())
        .args(["-f", "s16le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("start waveform decoder: {error}"))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let work = async {
        let mut bucketer = PeakBucketer::new(PEAK_COUNT, duration_ms * SAMPLE_RATE_HZ / 1000);
        let errors = async {
            let mut output = Vec::new();
            let mut buffer = [0_u8; 4096];
            let mut truncated = false;
            while let Ok(read) = stderr.read(&mut buffer).await {
                if read == 0 {
                    break;
                }
                let room = ERROR_OUTPUT_LIMIT_BYTES.saturating_sub(output.len());
                truncated |= read > room;
                output.extend_from_slice(&buffer[..read.min(room)]);
            }
            (output, truncated)
        };
        let (read_result, (error_output, truncated)) =
            tokio::join!(read_pcm_stream(stdout, &mut bucketer), errors);
        let status = child.wait().await;
        (bucketer, read_result, error_output, truncated, status)
    };
    let (bucketer, read_result, error_output, truncated, status) =
        tokio::time::timeout(GENERATION_TIMEOUT, work)
            .await
            .map_err(|_| "waveform decode: context deadline exceeded".to_owned())?;
    read_result.map_err(|error| format!("read waveform samples: {error}"))?;
    let status = status.map_err(|error| format!("decode waveform: {error}"))?;
    if !status.success() {
        let mut details = String::from_utf8_lossy(&error_output).trim().to_owned();
        if truncated {
            details.push_str(" [truncated]");
        }
        let failure = format!("decode waveform: {status}");
        return Err(if details.is_empty() {
            failure
        } else {
            format!("{failure}: {details}")
        });
    }
    Ok(bucketer.finish())
}

type Generation = Shared<BoxFuture<'static, Result<Arc<Vec<u8>>, Arc<String>>>>;

/// Serves cached peaks and generates a missing waveform once per Track at a
/// time, answering pending when generation outlasts a request.
pub struct WaveformService {
    repo: Repository,
    available: bool,
    pending_after: Duration,
    in_flight: Arc<Mutex<HashMap<String, Generation>>>,
    semaphore: Arc<Semaphore>,
}

impl WaveformService {
    /// `available` reflects the ffmpeg Server Dependency.
    pub fn new(pool: crate::db::Db, available: bool) -> Self {
        Self {
            repo: Repository::new(pool),
            available,
            pending_after: PENDING_AFTER,
            in_flight: Arc::default(),
            semaphore: Arc::new(Semaphore::new(CONCURRENCY)),
        }
    }

    pub async fn get(&self, track_id: &str) -> Result<TrackWaveform, WaveformError> {
        if !self.available {
            return Err(WaveformError::Unavailable);
        }
        let source = self.repo.get_track_waveform_source(track_id).await?;
        let metadata = match tokio::fs::metadata(&source.file_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(LibraryError::NotFound.into());
            }
            Err(error) => {
                return Err(WaveformError::Generation(format!(
                    "stat waveform source: {error}"
                )));
            }
        };
        let size = metadata.len() as i64;
        let modified = metadata.mtime();
        if let Some(cached) = self.repo.get_track_waveform(track_id).await?
            && cached.source_size_bytes == size
            && cached.source_modified_at == modified
        {
            return Ok(to_waveform(track_id, &cached.peaks));
        }
        let generation = self.generation(
            track_id,
            source.file_path,
            source.duration_ms,
            size,
            modified,
        );
        tokio::select! {
            result = generation => result
                .map(|peaks| to_waveform(track_id, &peaks))
                .map_err(|error| WaveformError::Generation(error.to_string())),
            _ = tokio::time::sleep(self.pending_after) => Err(WaveformError::Pending),
        }
    }

    /// The generation running for `track_id`, starting one if none is. It
    /// runs on its own task, so it finishes for the next caller even when
    /// the request that started it goes away.
    fn generation(
        &self,
        track_id: &str,
        path: String,
        duration_ms: i64,
        size: i64,
        modified: i64,
    ) -> Generation {
        let mut in_flight = self.in_flight.lock().expect("waveform generations lock");
        if let Some(existing) = in_flight.get(track_id) {
            return existing.clone();
        }
        let repo = self.repo.clone();
        let semaphore = self.semaphore.clone();
        let registry = self.in_flight.clone();
        let id = track_id.to_owned();
        let work = async move {
            let result = async {
                let queued = std::time::Instant::now();
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|error| error.to_string())?;
                let started = std::time::Instant::now();
                let peaks = generate_with_ffmpeg(path, duration_ms).await?;
                let generated = started.elapsed();
                observability::distribution_ms("waveform.generate.duration", generated, &[]);
                observability::record_span(
                    queued,
                    "waveform.generate",
                    queued.elapsed(),
                    &[],
                    &observability::sequential(&[
                        ("waveform.queue", Some(started - queued)),
                        ("waveform.ffmpeg", Some(generated)),
                    ]),
                );
                let record = WaveformRecord {
                    peaks,
                    source_size_bytes: size,
                    source_modified_at: modified,
                };
                repo.put_track_waveform(&id, &record)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(record.peaks)
            }
            .await;
            registry
                .lock()
                .expect("waveform generations lock")
                .remove(&id);
            result
        };
        // The request's own hub, so the generation it waits for is its span.
        let task = tokio::spawn(work.bind_hub(sentry::Hub::current()));
        let shared = async move {
            match task.await {
                Ok(Ok(peaks)) => Ok(Arc::new(peaks)),
                Ok(Err(error)) => Err(Arc::new(error)),
                Err(error) => Err(Arc::new(format!("waveform generation stopped: {error}"))),
            }
        }
        .boxed()
        .shared();
        in_flight.insert(track_id.to_owned(), shared.clone());
        shared
    }
}

fn to_waveform(track_id: &str, peaks: &[u8]) -> TrackWaveform {
    TrackWaveform {
        track_id: track_id.to_owned(),
        peak_count: peaks.len(),
        peaks: peaks.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn keeps_the_loudest_sample_per_bin_and_normalizes() {
        let peaks = bucket_peaks(&pcm(&[1, -4, 2, 2, -8, 0, 0, 1]), 4, 8);
        assert_eq!(peaks, [127, 63, 255, 31]);
    }

    #[test]
    fn unknown_lengths_bucket_at_the_end() {
        assert_eq!(bucket_peaks(&pcm(&[0, 10, 0, 5]), 2, 0), [255, 127]);
    }

    #[test]
    fn silence_is_all_zeros() {
        assert_eq!(bucket_peaks(&pcm(&[0, 0, 0, 0]), 4, 4), [0, 0, 0, 0]);
        assert_eq!(bucket_peaks(&[], 3, 0), [0, 0, 0]);
    }

    #[test]
    fn i16_min_does_not_overflow() {
        assert_eq!(bucket_peaks(&pcm(&[i16::MIN, 1]), 2, 2), [255, 0]);
    }

    #[tokio::test]
    async fn odd_chunk_boundaries_carry_the_low_byte_over() {
        let data = pcm(&[3, -6, 9]);
        let mut bucketer = PeakBucketer::new(3, 3);
        let reader = tokio_test_reader(&data);
        read_pcm_stream(reader, &mut bucketer).await.expect("read");
        assert_eq!(bucketer.finish(), [85, 170, 255]);
    }

    /// A reader that yields one byte at a time.
    fn tokio_test_reader(data: &[u8]) -> impl AsyncRead + Unpin {
        struct OneByte(std::collections::VecDeque<u8>);
        impl AsyncRead for OneByte {
            fn poll_read(
                mut self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                if let Some(byte) = self.0.pop_front() {
                    buf.put_slice(&[byte]);
                }
                std::task::Poll::Ready(Ok(()))
            }
        }
        OneByte(data.iter().copied().collect())
    }
}
