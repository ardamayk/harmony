//! WAV (wav_inspector.go): integer PCM in a RIFF WAVE container with an ID3
//! chunk for identity metadata. Every chunk must lie inside the file and the
//! data chunk must hold whole frames that read to the end.

use super::errors::{Code, Failure, Result, error, io_message, optional_artwork};
use super::metadata::{self, ReplayGain, Tags};
use super::{Context, MediaInspection, TechnicalAudio, audio_error, file_error, hash_file, id3};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const PCM: u16 = 0x0001;
const EXTENSIBLE: u16 = 0xfffe;
const RIFF_HEADER_SIZE_BYTES: u64 = 12;
const FMT_MIN_SIZE_BYTES: u64 = 16;
const FMT_WAVEFORMATEX_SIZE_BYTES: u64 = 18;
const FMT_EXTENSIBLE_SIZE_BYTES: u64 = 40;
const PCM_SUBFORMAT: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];

#[derive(Default)]
struct Format {
    audio_format: u16,
    channel_count: u16,
    sample_rate_hz: u32,
    bytes_per_second: u32,
    block_align: u16,
    bits_per_sample: u16,
    cb_size: u16,
    valid_bits: u16,
    sub_format: [u8; 16],
}

struct Chunks {
    format: Format,
    data_offset: u64,
    data_size: u64,
    tags: Tags,
    pictures: Vec<id3::TaggedPicture>,
}

fn unsupported(message: impl std::fmt::Display) -> Failure {
    error(Code::UnsupportedFormat, "container", message)
}

pub fn inspect(file: &mut File, ctx: &mut Context<'_>) -> Result<MediaInspection> {
    let (file_sha256, size_bytes) = hash_file(file, ctx)?;
    let size_bytes = size_bytes as u64;
    let mut header = [0u8; RIFF_HEADER_SIZE_BYTES as usize];
    file.read_exact(&mut header)
        .map_err(|cause| file_error(format!("read WAV header: {}", io_message(&cause))))?;
    let declared = u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap()));
    if declared + RIFF_HEADER_SIZE_BYTES - 4 != size_bytes {
        return Err(unsupported(format!(
            "RIFF size {declared} does not match the file size {size_bytes}"
        )));
    }
    let chunks = read_chunks(file, size_bytes)?;
    let normalized = metadata::normalize(&chunks.tags, ReplayGain::default());
    let artwork = optional_artwork(id3::front_cover_artwork(&chunks.pictures));
    let audio = decode_pcm(file, ctx, &chunks);
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

/// Walks every RIFF chunk once; overlapping bounds or a missing fmt, data
/// or ID3 chunk reject the file before any decoding.
fn read_chunks(file: &mut File, size_bytes: u64) -> Result<Chunks> {
    file.seek(SeekFrom::Start(RIFF_HEADER_SIZE_BYTES))
        .map_err(|cause| file_error(format!("seek past WAV header: {}", io_message(&cause))))?;
    let mut format: Option<Format> = None;
    let mut data: Option<(u64, u64)> = None;
    let mut id3_values: Option<(Tags, Vec<id3::TaggedPicture>)> = None;
    let mut offset = RIFF_HEADER_SIZE_BYTES;
    loop {
        let mut header = [0u8; 8];
        file.read_exact(&mut header).map_err(|cause| {
            unsupported(format!("read RIFF chunk header: {}", io_message(&cause)))
        })?;
        let id = String::from_utf8_lossy(&header[..4]).into_owned();
        let size = u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap()));
        let body_offset = offset + 8;
        if body_offset + size > size_bytes {
            return Err(unsupported(format!(
                "RIFF chunk {} extends past the file",
                go_quote(&id)
            )));
        }
        match &header[..4] {
            b"fmt " => {
                let parsed = read_format(file, size)?;
                if format.is_some() {
                    return Err(unsupported("multiple fmt chunks are ambiguous"));
                }
                format = Some(parsed);
            }
            b"data" => {
                if data.is_some() {
                    return Err(unsupported("multiple data chunks are ambiguous"));
                }
                data = Some((body_offset, size));
            }
            b"ID3 " | b"id3 " => {
                let parsed = read_id3(file, size)?;
                if id3_values.is_some() {
                    return Err(error(
                        Code::InvalidMetadata,
                        "metadata",
                        "multiple ID3 chunks are ambiguous",
                    ));
                }
                id3_values = Some(parsed);
            }
            _ => {}
        }
        let mut next = body_offset + size;
        if size % 2 == 1 {
            if next >= size_bytes {
                return Err(unsupported(format!(
                    "RIFF chunk {} is missing its padding byte",
                    go_quote(&id)
                )));
            }
            next += 1;
        }
        if next >= size_bytes {
            break;
        }
        file.seek(SeekFrom::Start(next))
            .map_err(|cause| file_error(format!("seek RIFF chunk: {}", io_message(&cause))))?;
        offset = next;
    }
    let format = format.ok_or_else(|| unsupported("WAV fmt chunk is missing"))?;
    let (data_offset, data_size) = data.unwrap_or_default();
    if data_size == 0 {
        return Err(audio_error("WAV data chunk is missing or empty"));
    }
    let (tags, pictures) = id3_values.ok_or_else(|| {
        error(
            Code::InvalidMetadata,
            "metadata",
            "WAV ID3 chunk is missing; identity metadata is required",
        )
    })?;
    Ok(Chunks {
        format,
        data_offset,
        data_size,
        tags,
        pictures,
    })
}

fn go_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn read_format(file: &mut File, size: u64) -> Result<Format> {
    if !(FMT_MIN_SIZE_BYTES..=FMT_EXTENSIBLE_SIZE_BYTES).contains(&size) {
        return Err(unsupported(format!("fmt chunk size {size} is invalid")));
    }
    let mut body = vec![0u8; size as usize];
    file.read_exact(&mut body)
        .map_err(|cause| file_error(format!("read fmt chunk: {}", io_message(&cause))))?;
    let u16_at = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(body[at..at + 4].try_into().unwrap());
    let mut format = Format {
        audio_format: u16_at(0),
        channel_count: u16_at(2),
        sample_rate_hz: u32_at(4),
        bytes_per_second: u32_at(8),
        block_align: u16_at(12),
        bits_per_sample: u16_at(14),
        ..Format::default()
    };
    if size >= FMT_WAVEFORMATEX_SIZE_BYTES {
        format.cb_size = u16_at(16);
    }
    if format.audio_format == EXTENSIBLE {
        if size < FMT_EXTENSIBLE_SIZE_BYTES {
            return Err(unsupported("extensible fmt chunk is truncated"));
        }
        format.valid_bits = u16_at(18);
        format.sub_format.copy_from_slice(&body[24..40]);
    } else if size != FMT_MIN_SIZE_BYTES
        && (size != FMT_WAVEFORMATEX_SIZE_BYTES || format.cb_size != 0)
    {
        return Err(unsupported(format!("PCM fmt chunk size {size} is invalid")));
    }
    validate_format(&format)?;
    Ok(format)
}

fn validate_format(format: &Format) -> Result<()> {
    let mut audio_format = format.audio_format;
    if audio_format == EXTENSIBLE {
        if u64::from(format.cb_size) != FMT_EXTENSIBLE_SIZE_BYTES - FMT_WAVEFORMATEX_SIZE_BYTES {
            return Err(unsupported(format!(
                "extensible fmt extension size {} is invalid",
                format.cb_size
            )));
        }
        if format.sub_format != PCM_SUBFORMAT {
            return Err(unsupported("WAV subformat is not integer PCM"));
        }
        audio_format = PCM;
    }
    if audio_format != PCM {
        return Err(unsupported(format!(
            "WAV audio format 0x{:04x} is not integer PCM",
            format.audio_format
        )));
    }
    if format.channel_count == 0 || format.channel_count > 8 {
        return Err(audio_error(format!(
            "channel count {} is invalid",
            format.channel_count
        )));
    }
    if !(8000..=384_000).contains(&format.sample_rate_hz) {
        return Err(audio_error(format!(
            "sample rate {} is invalid",
            format.sample_rate_hz
        )));
    }
    let bits = format.bits_per_sample;
    if !bits.is_multiple_of(8) || !(8..=32).contains(&bits) {
        return Err(audio_error(format!("bit depth {bits} is invalid")));
    }
    if u32::from(format.block_align) != u32::from(format.channel_count) * u32::from(bits / 8) {
        return Err(audio_error(format!(
            "block align {} does not match channels and bit depth",
            format.block_align
        )));
    }
    if u64::from(format.bytes_per_second)
        != u64::from(format.sample_rate_hz) * u64::from(format.block_align) % (1 << 32)
    {
        return Err(audio_error(
            "byte rate does not match sample rate and block align",
        ));
    }
    if format.audio_format == EXTENSIBLE && (format.valid_bits == 0 || format.valid_bits > bits) {
        return Err(audio_error(format!(
            "valid bits {} must be positive and no greater than container bit depth {}",
            format.valid_bits, bits
        )));
    }
    Ok(())
}

/// `parseWAVID3Chunk`: the shared strict ID3 parser over a bounded chunk.
fn read_id3(file: &mut File, size: u64) -> Result<(Tags, Vec<id3::TaggedPicture>)> {
    if size < id3::HEADER_SIZE_BYTES as u64
        || size > (id3::MAX_TAG_SIZE_BYTES + id3::HEADER_SIZE_BYTES) as u64
    {
        return Err(id3::invalid(format!("ID3 chunk size {size} is invalid")));
    }
    let mut body = vec![0u8; size as usize];
    file.read_exact(&mut body)
        .map_err(|cause| file_error(format!("read ID3 chunk: {}", io_message(&cause))))?;
    let (version, tag_size) = id3::parse_header(&body)?;
    if id3::HEADER_SIZE_BYTES + tag_size != body.len() {
        return Err(id3::invalid("ID3 tag size is invalid"));
    }
    let frames = id3::parse_frames(&body[id3::HEADER_SIZE_BYTES..], version)?;
    let (values, failure) = id3::collect_values(&frames, version);
    failure.into_result()?;
    Ok((values.tags, values.pictures))
}

/// Reads the whole data chunk: whole frames, every byte present.
fn decode_pcm(file: &mut File, ctx: &mut Context<'_>, chunks: &Chunks) -> Result<TechnicalAudio> {
    let format = &chunks.format;
    let block_align = u64::from(format.block_align);
    let size = chunks.data_size;
    if !size.is_multiple_of(block_align) {
        return Err(audio_error(format!(
            "data chunk size {size} is not a whole number of PCM frames"
        )));
    }
    let frame_count = size / block_align;
    file.seek(SeekFrom::Start(chunks.data_offset))
        .map_err(|cause| file_error(format!("seek WAV data chunk: {}", io_message(&cause))))?;
    ctx.check()?;
    let mut remaining = size;
    let mut buffer = vec![0u8; 1024 * 1024];
    while remaining > 0 {
        ctx.check()?;
        let want = remaining.min(buffer.len() as u64) as usize;
        let (read, failure) = read_full(file, &mut buffer[..want]);
        remaining -= read as u64;
        let read_frames = (size - remaining) / block_align;
        ctx.report(
            read_frames,
            frame_count,
            (size - remaining) as i64,
            size as i64,
            false,
        )?;
        if let Some(cause) = failure {
            return Err(audio_error(format!("read PCM samples: {cause}")));
        }
    }
    let duration_ms = (frame_count * 1000 / u64::from(format.sample_rate_hz)) as i64;
    if duration_ms <= 0 {
        return Err(audio_error("decoded duration is not positive"));
    }
    let bitrate_kbps = ((u64::from(format.bytes_per_second) * 8 + 500) / 1000) as i64;
    if bitrate_kbps <= 0 {
        return Err(audio_error("average encoded bitrate is not positive"));
    }
    ctx.report(frame_count, frame_count, size as i64, size as i64, true)?;
    let bit_depth = if format.valid_bits > 0 {
        format.valid_bits
    } else {
        format.bits_per_sample
    };
    Ok(TechnicalAudio {
        format: "wav".into(),
        container: "wav".into(),
        codec: if format.bits_per_sample == 8 {
            "pcm_u8".to_owned()
        } else {
            format!("pcm_s{:02}le", format.bits_per_sample)
        },
        duration_ms,
        sample_rate_hz: i64::from(format.sample_rate_hz),
        channel_count: i64::from(format.channel_count),
        bit_depth: i64::from(bit_depth),
        bitrate_kbps,
    })
}

/// `io.ReadFull`: the bytes read and why it stopped short.
fn read_full(file: &mut File, buffer: &mut [u8]) -> (usize, Option<String>) {
    let mut read = 0;
    while read < buffer.len() {
        match file.read(&mut buffer[read..]) {
            Ok(0) => {
                let cause = if read == 0 { "EOF" } else { "unexpected EOF" };
                return (read, Some(cause.to_owned()));
            }
            Ok(n) => read += n,
            Err(cause) if cause.kind() == std::io::ErrorKind::Interrupted => {}
            Err(cause) => return (read, Some(io_message(&cause))),
        }
    }
    (read, None)
}
