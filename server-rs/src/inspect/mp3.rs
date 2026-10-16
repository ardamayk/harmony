//! MP3 (mp3_inspector.go, mp3-apev2.go). The ID3v2 tag is required; every
//! MPEG Layer III frame up to an optional APEv2 tag and ID3v1 trailer must
//! parse and decode, one frame per packet like go-mp3 decodes them.

use super::errors::{Code, Failure, Result, error, io_message};
use super::{Context, MediaInspection, TechnicalAudio, audio_error, file_error, hash_file, id3};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::FileExt;
use symphonia::core::codecs::audio::well_known::CODEC_ID_MP3;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration, Timestamp};
use symphonia::default::codecs::MpaDecoder;

const ID3V1_TAG_SIZE_BYTES: u64 = 128;
const BASE_SAMPLE_RATES: [u32; 3] = [44_100, 48_000, 32_000];
const MPEG1_BITRATES: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const MPEG2_BITRATES: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
const MPEG_VERSION_2_5: u32 = 0;
const MPEG_VERSION_RESERVED: u32 = 1;
const MPEG_VERSION_2: u32 = 2;
const MPEG_VERSION_1: u32 = 3;

#[derive(Clone, Copy)]
struct FrameHeader {
    /// go-mp3 cannot decode MPEG 2.5 or reserved emphasis, so the Go server
    /// rejects such frames at decode; kept for parity until the cutover.
    go_mp3_unsupported: bool,
    sample_rate_hz: u32,
    channel_count: u32,
    frame_size: u64,
    samples: u64,
}

struct StreamInfo {
    sample_rate_hz: u32,
    channel_count: u32,
    total_samples: u64,
    encoded_bytes: i64,
    /// Where each frame starts and how long it is.
    frames: Vec<(u64, u64)>,
    go_mp3_unsupported: bool,
}

pub fn inspect(file: &mut File, ctx: &mut Context<'_>) -> Result<MediaInspection> {
    let (file_sha256, size_bytes) = hash_file(file, ctx)?;
    let tag = id3::inspect_tag(&mut BufReader::new(&mut *file))?;
    let stream = inspect_frames(file, ctx, tag.offset, size_bytes as u64);
    let audio = match stream {
        Ok(stream) => decode(file, ctx, &stream),
        Err(failure) => Err(failure),
    };
    Failure::join([Some(tag.failure), audio.as_ref().err().cloned()]).into_result()?;
    Ok(MediaInspection {
        metadata: tag.metadata?,
        album_artwork: tag.artwork,
        audio: audio?,
        file_sha256,
    })
}

fn inspect_frames(
    file: &mut File,
    ctx: &Context<'_>,
    audio_offset: u64,
    size_bytes: u64,
) -> Result<StreamInfo> {
    let audio_end = audio_end(file, size_bytes).map_err(file_error)?;
    let audio_end = apev2_audio_end(file, ctx, audio_offset, audio_end)
        .map_err(|cause| error(Code::InvalidMetadata, "APEv2", cause))?;
    if audio_offset >= audio_end {
        return Err(audio_error("MP3 audio stream is empty"));
    }
    file.seek(SeekFrom::Start(audio_offset))
        .map_err(|cause| file_error(format!("seek MP3 audio: {}", io_message(&cause))))?;
    let mut reader = BufReader::new(&mut *file);
    let mut stream = StreamInfo {
        sample_rate_hz: 0,
        channel_count: 0,
        total_samples: 0,
        encoded_bytes: 0,
        frames: Vec::new(),
        go_mp3_unsupported: false,
    };
    let mut offset = audio_offset;
    while offset < audio_end {
        ctx.check()?;
        let mut header = [0u8; 4];
        reader.read_exact(&mut header).map_err(|cause| {
            audio_error(format!("read MP3 frame header: {}", io_message(&cause)))
        })?;
        let header = match parse_frame_header(&header) {
            Some(header) if header.frame_size <= audio_end - offset => header,
            _ => return Err(audio_error("MP3 frame is invalid or truncated")),
        };
        if stream.total_samples == 0 {
            stream.sample_rate_hz = header.sample_rate_hz;
            stream.channel_count = header.channel_count;
        } else if stream.sample_rate_hz != header.sample_rate_hz
            || stream.channel_count != header.channel_count
        {
            return Err(audio_error("MP3 stream properties change between frames"));
        }
        stream.total_samples += header.samples;
        stream.go_mp3_unsupported |= header.go_mp3_unsupported;
        reader
            .seek_relative(header.frame_size as i64 - 4)
            .map_err(|cause| file_error(format!("skip MP3 frame: {}", io_message(&cause))))?;
        stream.frames.push((offset, header.frame_size));
        stream.encoded_bytes += header.frame_size as i64;
        offset += header.frame_size;
    }
    Ok(stream)
}

/// The end of the audio before an ID3v1 trailer.
fn audio_end(file: &File, size_bytes: u64) -> std::result::Result<u64, String> {
    if size_bytes < ID3V1_TAG_SIZE_BYTES {
        return Ok(size_bytes);
    }
    let mut signature = [0u8; 3];
    file.read_exact_at(&mut signature, size_bytes - ID3V1_TAG_SIZE_BYTES)
        .map_err(|cause| format!("read ID3v1 signature: {}", io_message(&cause)))?;
    Ok(if &signature == b"TAG" {
        size_bytes - ID3V1_TAG_SIZE_BYTES
    } else {
        size_bytes
    })
}

fn parse_frame_header(data: &[u8; 4]) -> Option<FrameHeader> {
    let header = u32::from_be_bytes(*data);
    let version = (header >> 19) & 3;
    let layer = (header >> 17) & 3;
    let bitrate_index = ((header >> 12) & 15) as usize;
    let sample_rate_index = ((header >> 10) & 3) as usize;
    if header & 0xffe0_0000 != 0xffe0_0000
        || version == MPEG_VERSION_RESERVED
        || layer != 1
        || bitrate_index == 0
        || bitrate_index == 15
        || sample_rate_index == 3
    {
        return None;
    }
    let base = BASE_SAMPLE_RATES[sample_rate_index];
    let sample_rate_hz = match version {
        MPEG_VERSION_2 => base / 2,
        MPEG_VERSION_2_5 => base / 4,
        _ => base,
    };
    let (bitrate_kbps, coefficient, samples) = if version == MPEG_VERSION_1 {
        (MPEG1_BITRATES[bitrate_index], 144, 1152)
    } else {
        (MPEG2_BITRATES[bitrate_index], 72, 576)
    };
    let padding = (header >> 9) & 1;
    let frame_size = coefficient * bitrate_kbps * 1000 / sample_rate_hz + padding;
    let channel_count = if (header >> 6) & 3 == 3 { 1 } else { 2 };
    Some(FrameHeader {
        go_mp3_unsupported: version == MPEG_VERSION_2_5 || header & 3 == 2,
        sample_rate_hz,
        channel_count,
        frame_size: u64::from(frame_size),
        samples,
    })
}

const APEV2_DESCRIPTOR_SIZE: u64 = 32;
const APEV2_HAS_HEADER: u32 = 1 << 31;
const APEV2_IS_HEADER: u32 = 1 << 29;
const APEV2_ITEM_FLAGS: u32 = 7;
const APEV2_MIN_ITEM_SIZE: u64 = 11;

/// Only a trailing APEv2 tag is accepted, and only its boundaries are
/// checked; ID3 stays the metadata source.
fn apev2_audio_end(
    file: &File,
    ctx: &Context<'_>,
    audio_offset: u64,
    audio_end: u64,
) -> std::result::Result<u64, String> {
    if audio_end - audio_offset < APEV2_DESCRIPTOR_SIZE {
        return Ok(audio_end);
    }
    let mut footer = [0u8; APEV2_DESCRIPTOR_SIZE as usize];
    file.read_exact_at(&mut footer, audio_end - APEV2_DESCRIPTOR_SIZE)
        .map_err(|cause| format!("read APEv2 footer: {}", io_message(&cause)))?;
    if &footer[..8] != b"APETAGEX" {
        return Ok(audio_end);
    }
    validate_apev2_descriptor(&footer, false)?;
    let le = |range: std::ops::Range<usize>| {
        u64::from(u32::from_le_bytes(footer[range].try_into().unwrap()))
    };
    let size = le(12..16);
    let count = le(16..20);
    if size < APEV2_DESCRIPTOR_SIZE
        || size > audio_end - audio_offset
        || count > (size - APEV2_DESCRIPTOR_SIZE) / APEV2_MIN_ITEM_SIZE
    {
        return Err("APEv2 size or item count exceeds tag boundaries".to_owned());
    }
    let mut start = audio_end - size;
    validate_apev2_items(file, ctx, start, size - APEV2_DESCRIPTOR_SIZE, count)?;
    if le(20..24) as u32 & APEV2_HAS_HEADER != 0 {
        if start < audio_offset + APEV2_DESCRIPTOR_SIZE {
            return Err("APEv2 header overlaps ID3 or audio start".to_owned());
        }
        start -= APEV2_DESCRIPTOR_SIZE;
        let mut header = [0u8; APEV2_DESCRIPTOR_SIZE as usize];
        file.read_exact_at(&mut header, start)
            .map_err(|cause| format!("read APEv2 header: {}", io_message(&cause)))?;
        validate_apev2_descriptor(&header, true)?;
        let flags = u32::from_le_bytes(header[20..24].try_into().unwrap()) & !APEV2_IS_HEADER;
        header[20..24].copy_from_slice(&flags.to_le_bytes());
        if header != footer {
            return Err("APEv2 header and footer disagree".to_owned());
        }
    }
    Ok(start)
}

fn validate_apev2_descriptor(data: &[u8], is_header: bool) -> std::result::Result<(), String> {
    let flags = u32::from_le_bytes(data[20..24].try_into().unwrap());
    let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
    if &data[..8] != b"APETAGEX" || version != 2000 {
        return Err("APEv2 signature or version is invalid".to_owned());
    }
    let allowed = APEV2_HAS_HEADER | APEV2_IS_HEADER | APEV2_ITEM_FLAGS;
    if flags & !allowed != 0
        || (flags & APEV2_IS_HEADER != 0) != is_header
        || flags & 6 == 6
        || data[24..].iter().any(|byte| *byte != 0)
    {
        return Err("APEv2 flags or reserved bytes are invalid".to_owned());
    }
    Ok(())
}

/// Walks the item lengths without reading the values.
fn validate_apev2_items(
    file: &File,
    ctx: &Context<'_>,
    offset: u64,
    size: u64,
    count: u64,
) -> std::result::Result<(), String> {
    let mut data = vec![0u8; size as usize];
    if ctx.is_cancelled() {
        return Err("read APEv2 item header: context canceled".to_owned());
    }
    file.read_exact_at(&mut data, offset)
        .map_err(|cause| format!("read APEv2 item header: {}", io_message(&cause)))?;
    let mut position = 0usize;
    for _ in 0..count {
        if data.len() - position < 8 {
            let cause = if data.len() == position {
                "EOF"
            } else {
                "unexpected EOF"
            };
            return Err(format!("read APEv2 item header: {cause}"));
        }
        let value_size =
            u32::from_le_bytes(data[position..position + 4].try_into().unwrap()) as usize;
        let flags = u32::from_le_bytes(data[position + 4..position + 8].try_into().unwrap());
        position += 8;
        if flags & !APEV2_ITEM_FLAGS != 0 || flags & 6 == 6 {
            return Err("APEv2 item flags are invalid".to_owned());
        }
        let mut length = 0;
        loop {
            let Some(&byte) = data.get(position) else {
                return Err("APEv2 item key is truncated: EOF".to_owned());
            };
            position += 1;
            if byte == 0 && length >= 2 {
                break;
            }
            if !(b' '..=b'~').contains(&byte) {
                return Err("APEv2 item key is invalid".to_owned());
            }
            length += 1;
            if length > 255 {
                return Err("APEv2 item key exceeds length limit".to_owned());
            }
        }
        if data.len() - position < value_size {
            return Err("APEv2 item exceeds tag boundaries: EOF".to_owned());
        }
        position += value_size;
    }
    if position != data.len() {
        return Err("APEv2 item count does not match tag size".to_owned());
    }
    Ok(())
}

fn decode(file: &File, ctx: &mut Context<'_>, stream: &StreamInfo) -> Result<TechnicalAudio> {
    let mut params = AudioCodecParameters::new();
    params.codec = CODEC_ID_MP3;
    let mut options = AudioDecoderOptions::default();
    options.gapless = false;
    options.verify = false;
    let mut decoder = MpaDecoder::try_new(&params, &options).map_err(audio_error)?;
    if stream.go_mp3_unsupported {
        return Err(audio_error(
            "MPEG 2.5 or reserved-emphasis frames are not supported",
        ));
    }
    let mut decoded_samples = 0u64;
    let mut buffer = Vec::new();
    for (index, (offset, size)) in stream.frames.iter().enumerate() {
        ctx.check()?;
        buffer.resize(*size as usize, 0);
        file.read_exact_at(&mut buffer, *offset)
            .map_err(|cause| audio_error(io_message(&cause)))?;
        let packet = Packet::new(
            0,
            Timestamp::new(index as i64),
            Duration::new(0),
            buffer.as_slice(),
        );
        let decoded = decoder.decode(&packet).map_err(audio_error)?;
        if decoded.spec().rate() != stream.sample_rate_hz {
            return Err(audio_error(
                "decoded MP3 sample count or sample rate is inconsistent",
            ));
        }
        decoded_samples += decoded.frames() as u64;
        ctx.report(decoded_samples, stream.total_samples, 0, 0, false)?;
    }
    ctx.report(decoded_samples, stream.total_samples, 0, 0, true)?;
    if decoded_samples != stream.total_samples {
        return Err(audio_error(
            "decoded MP3 sample count or sample rate is inconsistent",
        ));
    }
    let duration_ms = (decoded_samples * 1000 / u64::from(stream.sample_rate_hz)) as i64;
    let bitrate_kbps = if duration_ms > 0 {
        (stream.encoded_bytes * 8 + duration_ms / 2) / duration_ms
    } else {
        0
    };
    if duration_ms <= 0 || bitrate_kbps <= 0 {
        return Err(audio_error("decoded MP3 technical properties are invalid"));
    }
    Ok(TechnicalAudio {
        format: "mp3".into(),
        container: "mp3".into(),
        codec: "mp3".into(),
        duration_ms,
        sample_rate_hz: i64::from(stream.sample_rate_hz),
        channel_count: i64::from(stream.channel_count),
        bit_depth: 0,
        bitrate_kbps,
    })
}
