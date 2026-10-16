//! Ogg Vorbis and Ogg Opus (ogg_inspector.go). Every page must carry a valid
//! checksum and continue the one logical stream through its end-of-stream
//! page. Vorbis decodes through symphonia; Opus packets are counted from
//! their table-of-contents bytes and ffmpeg decodes the stream.

use super::errors::{Code, Failure, Result, error, io_message, optional_artwork};
use super::metadata::{self, Tags, tag_key, tag_string};
use super::{
    AlbumArtwork, Context, MediaInspection, TechnicalAudio, artwork, audio_error,
    average_bitrate_kbps, file_error, flac, hash_and_rewind, m4a,
};
use base64::Engine;
use std::fs::File;
use std::io::{BufReader, ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;
use symphonia::core::codecs::audio::well_known::CODEC_ID_VORBIS;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration, Timestamp};
use symphonia::default::codecs::VorbisDecoder;

const PAGE_HEADER_SIZE_BYTES: usize = 27;
const END_OF_STREAM_FLAG: u8 = 0x04;
const BEGINNING_OF_STREAM_FLAG: u8 = 0x02;
const CONTINUED_PACKET_FLAG: u8 = 0x01;
const MAX_SEGMENT_SIZE: u8 = 255;
const OPUS_SAMPLE_RATE_HZ: u64 = 48_000;
const MAX_PICTURE_COMMENT_BYTES: usize = (artwork::MAX_ARTWORK_SIZE_BYTES + 1024) * 2;
const MAX_HEADER_PACKET_BYTES: i64 = MAX_PICTURE_COMMENT_BYTES as i64 + 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    Vorbis,
    Opus,
}

struct Page {
    header_type: u8,
    granule: u64,
    serial: u32,
    sequence: u32,
    segments: Vec<u8>,
    payload: Vec<u8>,
}

enum PageError {
    /// No bytes at all: the clean end of the stream.
    Eof,
    Invalid(String),
}

#[derive(Default)]
struct Analysis {
    encoded_audio_bytes: i64,
    final_granule: u64,
}

pub fn inspect(file: &mut File, path: &Path, ctx: &mut Context<'_>) -> Result<MediaInspection> {
    let (file_sha256, size_bytes) = hash_and_rewind(file, ctx).map_err(|cause| match cause {
        super::HashError::Cancelled => error(
            Code::ValidationCancelled,
            "validation",
            "hash file: context canceled",
        ),
        super::HashError::Io(message) => file_error(message),
    })?;
    let codec =
        detect_codec(file).map_err(|cause| error(Code::UnsupportedFormat, "container", cause))?;
    let header_packets = if codec == Codec::Opus { 2 } else { 3 };
    let analysis = analyze(file, ctx, header_packets).map_err(|cause| match cause {
        AnalysisError::Cancelled => {
            error(Code::ValidationCancelled, "validation", "context canceled")
        }
        AnalysisError::Invalid(message) => audio_error(message),
    })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|cause| file_error(format!("rewind OGG stream: {}", io_message(&cause))))?;
    let mut inspection = match codec {
        Codec::Vorbis => inspect_vorbis(file, ctx, size_bytes, &analysis)?,
        Codec::Opus => inspect_opus(file, path, ctx, size_bytes, &analysis)?,
    };
    inspection.file_sha256 = file_sha256;
    Ok(inspection)
}

fn read_page(reader: &mut impl Read) -> std::result::Result<Page, PageError> {
    let mut header = [0u8; PAGE_HEADER_SIZE_BYTES];
    let mut read = 0;
    while read < header.len() {
        match reader.read(&mut header[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(cause) if cause.kind() == ErrorKind::Interrupted => {}
            Err(cause) => return Err(PageError::Invalid(io_message(&cause))),
        }
    }
    if read == 0 {
        return Err(PageError::Eof);
    }
    if read < header.len() {
        return Err(PageError::Invalid("unexpected EOF".to_owned()));
    }
    if &header[..4] != b"OggS" || header[4] != 0 {
        return Err(PageError::Invalid("OGG page header is invalid".to_owned()));
    }
    let mut segments = vec![0u8; usize::from(header[26])];
    reader.read_exact(&mut segments).map_err(|cause| {
        PageError::Invalid(format!("read OGG segment table: {}", io_message(&cause)))
    })?;
    let payload_size: usize = segments.iter().map(|size| usize::from(*size)).sum();
    let mut payload = vec![0u8; payload_size];
    reader.read_exact(&mut payload).map_err(|cause| {
        PageError::Invalid(format!("read OGG page payload: {}", io_message(&cause)))
    })?;
    let mut unsummed = header;
    unsummed[22..26].fill(0);
    let checksum = crc(crc(crc(0, &unsummed), &segments), &payload);
    if checksum != u32::from_le_bytes(header[22..26].try_into().unwrap()) {
        return Err(PageError::Invalid(
            "OGG page checksum is invalid".to_owned(),
        ));
    }
    Ok(Page {
        header_type: header[5],
        granule: u64::from_le_bytes(header[6..14].try_into().unwrap()),
        serial: u32::from_le_bytes(header[14..18].try_into().unwrap()),
        sequence: u32::from_le_bytes(header[18..22].try_into().unwrap()),
        segments,
        payload,
    })
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = (index as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04c1_1db7
            } else {
                value << 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

fn crc(mut checksum: u32, data: &[u8]) -> u32 {
    for byte in data {
        checksum = (checksum << 8) ^ CRC_TABLE[usize::from((checksum >> 24) as u8 ^ byte)];
    }
    checksum
}

fn detect_codec(file: &mut File) -> std::result::Result<Codec, String> {
    let page = match read_page(&mut BufReader::new(&mut *file)) {
        Ok(page) => page,
        Err(PageError::Eof) => return Err("read OGG page header: EOF".to_owned()),
        Err(PageError::Invalid(message)) => return Err(format!("read OGG page header: {message}")),
    };
    let mut size = 0;
    for segment in &page.segments {
        size += usize::from(*segment);
        if *segment < MAX_SEGMENT_SIZE {
            let packet = &page.payload[..size];
            return if packet.starts_with(b"\x01vorbis") {
                Ok(Codec::Vorbis)
            } else if packet.starts_with(b"OpusHead") {
                Ok(Codec::Opus)
            } else {
                Err("OGG stream codec is unsupported".to_owned())
            };
        }
    }
    Err("OGG identification packet spans pages".to_owned())
}

enum AnalysisError {
    Cancelled,
    Invalid(String),
}

/// `analyzeOGGStream`: page continuity, header packet sizes, the encoded
/// audio size and the final granule position.
fn analyze(
    file: &mut File,
    ctx: &Context<'_>,
    header_packets: usize,
) -> std::result::Result<Analysis, AnalysisError> {
    file.seek(SeekFrom::Start(0)).map_err(|cause| {
        AnalysisError::Invalid(format!(
            "rewind OGG stream analysis: {}",
            io_message(&cause)
        ))
    })?;
    let mut reader = BufReader::new(&mut *file);
    let mut analysis = Analysis::default();
    let (mut serial, mut expected_sequence) = (0u32, 0u32);
    let mut partial = 0i64;
    let mut packet_index = 0usize;
    let (mut has_page, mut has_eos) = (false, false);
    loop {
        if ctx.is_cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        let page = match read_page(&mut reader) {
            Ok(page) => page,
            Err(PageError::Eof) => break,
            Err(PageError::Invalid(message)) => {
                return Err(AnalysisError::Invalid(format!("read OGG page: {message}")));
            }
        };
        validate_continuity(
            &page,
            has_page,
            has_eos,
            serial,
            expected_sequence,
            partial > 0,
        )
        .map_err(|message| AnalysisError::Invalid(message.to_owned()))?;
        if !has_page {
            serial = page.serial;
        }
        has_page = true;
        expected_sequence = page.sequence.wrapping_add(1);
        for segment in &page.segments {
            partial += i64::from(*segment);
            if packet_index < header_packets && partial > MAX_HEADER_PACKET_BYTES {
                return Err(AnalysisError::Invalid(
                    "OGG header packet exceeds maximum size".to_owned(),
                ));
            }
            if *segment < MAX_SEGMENT_SIZE {
                if packet_index >= header_packets {
                    analysis.encoded_audio_bytes += partial;
                }
                partial = 0;
                packet_index += 1;
            }
        }
        if page.header_type & END_OF_STREAM_FLAG != 0 {
            has_eos = true;
            analysis.final_granule = page.granule;
        }
    }
    if !has_page
        || !has_eos
        || partial != 0
        || packet_index <= header_packets
        || analysis.encoded_audio_bytes <= 0
    {
        return Err(AnalysisError::Invalid(
            "OGG stream is incomplete".to_owned(),
        ));
    }
    Ok(analysis)
}

fn validate_continuity(
    page: &Page,
    has_page: bool,
    has_eos: bool,
    serial: u32,
    expected_sequence: u32,
    has_partial_packet: bool,
) -> std::result::Result<(), &'static str> {
    if has_eos {
        return Err("OGG data follows end-of-stream page");
    }
    if !has_page {
        if page.header_type & BEGINNING_OF_STREAM_FLAG == 0 || page.sequence != 0 {
            return Err("OGG beginning-of-stream page is invalid");
        }
    } else if page.serial != serial
        || page.sequence != expected_sequence
        || page.header_type & BEGINNING_OF_STREAM_FLAG != 0
    {
        return Err("OGG stream continuity is invalid");
    }
    if (page.header_type & CONTINUED_PACKET_FLAG != 0) != has_partial_packet {
        return Err("OGG continued-packet flag is invalid");
    }
    Ok(())
}

/// One packet with the page it completes on.
struct OggPacket {
    data: Vec<u8>,
    granule: u64,
    last_on_page: bool,
    is_last: bool,
}

/// The packets of an analysed stream, in order.
fn packets(file: &mut File) -> std::result::Result<Vec<OggPacket>, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|cause| io_message(&cause))?;
    let mut reader = BufReader::new(&mut *file);
    let mut out = Vec::new();
    let mut partial = Vec::new();
    loop {
        let page = match read_page(&mut reader) {
            Ok(page) => page,
            Err(PageError::Eof) => break,
            Err(PageError::Invalid(message)) => return Err(message),
        };
        let first_on_page = out.len();
        let mut offset = 0;
        for segment in &page.segments {
            let size = usize::from(*segment);
            partial.extend_from_slice(&page.payload[offset..offset + size]);
            offset += size;
            if *segment < MAX_SEGMENT_SIZE {
                out.push(OggPacket {
                    data: std::mem::take(&mut partial),
                    granule: page.granule,
                    last_on_page: false,
                    is_last: false,
                });
            }
        }
        if out.len() > first_on_page {
            let last = out.last_mut().unwrap();
            last.last_on_page = true;
            last.is_last = page.header_type & END_OF_STREAM_FLAG != 0;
        }
    }
    Ok(out)
}

/// `collectCommentTags`: every comment must be `KEY=value` with a key.
fn collect_comment_tags(comments: &[String]) -> Result<Tags> {
    let mut tags = Tags::new();
    for comment in comments {
        let (key, value) = match comment.split_once('=') {
            Some((key, value)) if !key.trim().is_empty() => (tag_key(key), value),
            _ => {
                return Err(error(
                    Code::InvalidMetadata,
                    "comments",
                    "vorbis comment is malformed",
                ));
            }
        };
        tags.entry(key).or_default().push(value.to_owned());
    }
    Ok(tags)
}

/// The Vorbis comment structure after the packet signature, strictly.
fn parse_comment_payload(payload: &[u8]) -> std::result::Result<Vec<String>, String> {
    let (_, mut remaining) = read_length_prefixed(payload)?;
    if remaining.len() < 4 {
        return Err("unexpected EOF".to_owned());
    }
    let count = u32::from_le_bytes(remaining[..4].try_into().unwrap()) as usize;
    remaining = &remaining[4..];
    if count > remaining.len() / 4 {
        return Err("vorbis comment count exceeds payload".to_owned());
    }
    let mut comments = Vec::with_capacity(count);
    for _ in 0..count {
        let (value, next) = read_length_prefixed(remaining)?;
        comments.push(tag_string(value));
        remaining = next;
    }
    Ok(comments)
}

fn read_length_prefixed(data: &[u8]) -> std::result::Result<(&[u8], &[u8]), String> {
    if data.len() < 4 {
        return Err("unexpected EOF".to_owned());
    }
    let length = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    if length > data.len() - 4 {
        return Err("unexpected EOF".to_owned());
    }
    Ok((&data[4..4 + length], &data[4 + length..]))
}

/// jfreymuth/vorbis reads a comment header leniently: a truncated header
/// keeps the comments read so far and empty ones for the rest.
fn lenient_vorbis_comments(header: &[u8]) -> Vec<String> {
    let read_u32 = |data: &[u8], at: usize| -> Option<usize> {
        data.get(at..at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()) as usize)
    };
    let Some(vendor) = read_u32(header, 0) else {
        return Vec::new();
    };
    let mut at = 4usize.saturating_add(vendor);
    let Some(count) = read_u32(header, at) else {
        return Vec::new();
    };
    if count > 1 << 24 {
        return Vec::new();
    }
    at += 4;
    let mut comments = vec![String::new(); count];
    for comment in comments.iter_mut() {
        let Some(length) = read_u32(header, at) else {
            break;
        };
        at += 4;
        let Some(value) = header.get(at..at.saturating_add(length)) else {
            break;
        };
        *comment = tag_string(value);
        at += length;
    }
    comments
}

fn picture_comments_artwork(values: &[String]) -> Result<AlbumArtwork> {
    let invalid = |message: String| error(Code::InvalidArtwork, "artwork", message);
    let mut cover: Option<flac::Picture> = None;
    for value in values {
        let picture = decode_picture_comment(value).map_err(invalid)?;
        if picture.picture_type != flac::PICTURE_TYPE_FRONT_COVER {
            continue;
        }
        if cover.is_some() {
            return Err(invalid("multiple front covers are ambiguous".to_owned()));
        }
        cover = Some(picture);
    }
    match cover {
        Some(picture) => artwork::validate(&picture.mime_type, &picture.data),
        None => Err(error(
            Code::MissingArtwork,
            "artwork",
            "embedded front cover is required",
        )),
    }
}

fn decode_picture_comment(value: &str) -> std::result::Result<flac::Picture, String> {
    if value.is_empty() || value.len() > MAX_PICTURE_COMMENT_BYTES {
        return Err("encoded picture comment size is invalid".to_owned());
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|cause| format!("decode picture comment: {cause}"))?;
    let picture = flac::parse_picture(&data)?;
    // The data length must cover exactly the rest of the block.
    let header = 32 + read_len(&data, 4) + read_len(&data, 8 + read_len(&data, 4));
    if picture.data.len() > artwork::MAX_ARTWORK_SIZE_BYTES
        || header + picture.data.len() != data.len()
    {
        return Err("picture data length is invalid".to_owned());
    }
    Ok(picture)
}

fn read_len(data: &[u8], at: usize) -> usize {
    data.get(at..at + 4).map_or(0, |bytes| {
        u32::from_be_bytes(bytes.try_into().unwrap()) as usize
    })
}

/// Metadata from the comments and the front cover from picture comments.
fn metadata_and_artwork(
    tags: &Tags,
) -> (Result<metadata::NormalizedMetadata>, Result<AlbumArtwork>) {
    let normalized = metadata::normalize(tags, metadata::replay_gain_from_tags(tags));
    let pictures = tags
        .get("METADATA_BLOCK_PICTURE")
        .map_or(&[][..], Vec::as_slice);
    (
        normalized,
        optional_artwork(picture_comments_artwork(pictures)),
    )
}

fn finish(
    normalized: Result<metadata::NormalizedMetadata>,
    artwork: Result<AlbumArtwork>,
    audio: Result<TechnicalAudio>,
) -> Result<MediaInspection> {
    let metadata_failure = Failure::join([
        normalized.as_ref().err().cloned(),
        artwork.as_ref().err().cloned(),
    ]);
    if let Err(failure) = &audio {
        return Err(Failure::join([
            Some(metadata_failure),
            Some(failure.clone()),
        ]));
    }
    metadata_failure.into_result()?;
    Ok(MediaInspection {
        metadata: normalized?,
        album_artwork: artwork?,
        audio: audio?,
        file_sha256: String::new(),
    })
}

fn inspect_vorbis(
    file: &mut File,
    ctx: &mut Context<'_>,
    size_bytes: i64,
    analysis: &Analysis,
) -> Result<MediaInspection> {
    let packets = packets(file).map_err(audio_error)?;
    let comments_error = |message: &str| error(Code::InvalidMetadata, "comments", message);
    let ident = &packets[0].data;
    if ident.len() <= 7 + 22 || ident[7..11] != [0, 0, 0, 0] || ident[7 + 22] & 1 == 0 {
        return Err(comments_error("vorbis: decoding error"));
    }
    let comment = packets
        .get(1)
        .map(|packet| &packet.data[..])
        .unwrap_or_default();
    let is_header = comment.len() > 6 && comment[0] & 1 == 1 && &comment[1..7] == b"vorbis";
    if !is_header {
        return Err(comments_error("vorbis: invalid header"));
    }
    let comments = match comment[0] {
        3 => lenient_vorbis_comments(&comment[7..]),
        1 | 5 => Vec::new(),
        _ => return Err(comments_error("vorbis: unknown header type")),
    };
    let tags = collect_comment_tags(&comments)?;
    let (normalized, artwork) = metadata_and_artwork(&tags);
    let audio = decode_vorbis(ctx, &packets, size_bytes, analysis);
    finish(normalized, artwork, audio)
}

/// Decodes every audio packet and counts samples the way oggvorbis does:
/// the start is trimmed to the first complete page's granule position and
/// the end to the final granule position.
fn decode_vorbis(
    ctx: &mut Context<'_>,
    packets: &[OggPacket],
    size_bytes: i64,
    analysis: &Analysis,
) -> Result<TechnicalAudio> {
    let mut extra = packets[0].data.clone();
    extra.extend_from_slice(&packets[2].data);
    let mut params = AudioCodecParameters::new();
    params.codec = CODEC_ID_VORBIS;
    params.with_extra_data(extra.into_boxed_slice());
    let mut options = AudioDecoderOptions::default();
    options.gapless = false;
    options.verify = false;
    let mut decoder = VorbisDecoder::try_new(&params, &options).map_err(audio_error)?;
    let ident = &packets[0].data;
    let channels = i64::from(ident[11]);
    let sample_rate = i64::from(u32::from_le_bytes(ident[12..16].try_into().unwrap()));
    if channels <= 0 || sample_rate <= 0 {
        return Err(audio_error(
            "vorbis stream has invalid technical properties",
        ));
    }
    let length = analysis.final_granule as i64;
    let mut position = 0i64;
    let mut decoded_samples = 0i64;
    let mut started = false;
    for (index, packet) in packets.iter().enumerate().skip(3) {
        ctx.check()?;
        let source = Packet::new(
            0,
            Timestamp::new(index as i64),
            Duration::new(0),
            packet.data.as_slice(),
        );
        let decoded = decoder.decode(&source).map_err(audio_error)?.frames() as i64;
        // The first audio packet only primes the overlap; oggvorbis and the
        // Vorbis I spec output nothing for it, where symphonia does.
        let mut frames = if index == 3 { 0 } else { decoded };
        position += frames;
        if !started && frames == 0 {
            continue;
        }
        if packet.is_last {
            let discard = position - packet.granule as i64;
            if discard > 0 {
                frames -= discard;
            }
            position = packet.granule as i64;
        }
        if !started {
            started = true;
            if packet.last_on_page {
                let offset = packet.granule as i64 - position;
                if offset < 0 {
                    frames += offset;
                }
                position = packet.granule as i64;
            }
        }
        if frames < 0 {
            return Err(audio_error(
                "vorbis packet trims more samples than it decoded",
            ));
        }
        decoded_samples += frames;
        ctx.report(
            decoded_samples as u64,
            length.max(0) as u64,
            0,
            size_bytes,
            false,
        )?;
    }
    if length <= 0 || decoded_samples != length {
        return Err(audio_error(
            "decoded sample count does not match OGG granule position",
        ));
    }
    let duration_ms = decoded_samples * 1000 / sample_rate;
    let bitrate_kbps = average_bitrate_kbps(analysis.encoded_audio_bytes, duration_ms);
    if duration_ms <= 0 || bitrate_kbps <= 0 {
        return Err(audio_error("vorbis duration or bitrate is not positive"));
    }
    ctx.report(
        decoded_samples as u64,
        decoded_samples as u64,
        size_bytes,
        size_bytes,
        true,
    )?;
    Ok(TechnicalAudio {
        format: "ogg".into(),
        container: "ogg".into(),
        codec: "vorbis".into(),
        duration_ms,
        sample_rate_hz: sample_rate,
        channel_count: channels,
        bit_depth: 0,
        bitrate_kbps,
    })
}

struct OpusHead {
    version: u8,
    channels: u8,
    pre_skip: u64,
    mapping: u8,
}

fn inspect_opus(
    file: &mut File,
    path: &Path,
    ctx: &mut Context<'_>,
    size_bytes: i64,
    analysis: &Analysis,
) -> Result<MediaInspection> {
    let packets =
        packets(file).map_err(|cause| error(Code::UnsupportedFormat, "container", cause))?;
    let head = &packets[0].data;
    if head.len() < 19 {
        return Err(error(
            Code::UnsupportedFormat,
            "container",
            "bad header signature",
        ));
    }
    let head = OpusHead {
        version: head[8],
        channels: head[9],
        pre_skip: u64::from(u16::from_le_bytes([head[10], head[11]])),
        mapping: head[18],
    };
    if head.version != 1 || head.channels == 0 || head.channels > 2 || head.mapping != 0 {
        return Err(error(
            Code::UnsupportedFormat,
            "audio",
            "opus stream channel layout is unsupported",
        ));
    }
    let comment = packets
        .get(1)
        .map(|packet| &packet.data[..])
        .unwrap_or_default();
    let Some(payload) = comment.strip_prefix(b"OpusTags") else {
        return Err(error(
            Code::InvalidMetadata,
            "comments",
            "opus comment header is invalid",
        ));
    };
    let comments = parse_comment_payload(payload)
        .map_err(|cause| error(Code::InvalidMetadata, "comments", cause))?;
    let tags = collect_comment_tags(&comments)?;
    let (normalized, artwork) = metadata_and_artwork(&tags);
    let audio = decode_opus(ctx, &packets, &head, size_bytes, analysis, path);
    finish(normalized, artwork, audio)
}

/// Samples in one Opus packet at 48 kHz, from its table of contents (RFC
/// 6716 section 3.1).
fn opus_packet_samples(packet: &[u8]) -> Option<u64> {
    let toc = *packet.first()?;
    let config = toc >> 3;
    let frame_samples: u64 = match config {
        0..=11 => [480, 960, 1920, 2880][usize::from(config % 4)],
        12..=15 => [480, 960][usize::from(config % 2)],
        _ => [120, 240, 480, 960][usize::from(config % 4)],
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => u64::from(*packet.get(1)? & 0x3f),
    };
    let samples = frames * frame_samples;
    (frames > 0 && samples <= 5760).then_some(samples)
}

fn decode_opus(
    ctx: &mut Context<'_>,
    packets: &[OggPacket],
    head: &OpusHead,
    size_bytes: i64,
    analysis: &Analysis,
    path: &Path,
) -> Result<TechnicalAudio> {
    let mut decoded_samples = 0u64;
    let mut encoded_bytes = 0i64;
    let mut final_granule = 0u64;
    for packet in &packets[2..] {
        ctx.check()?;
        let samples = opus_packet_samples(&packet.data)
            .ok_or_else(|| audio_error("opus packet decoded no samples"))?;
        decoded_samples += samples;
        encoded_bytes += packet.data.len() as i64;
        final_granule = packet.granule;
        ctx.report(decoded_samples, 0, encoded_bytes, size_bytes, false)?;
    }
    if encoded_bytes != analysis.encoded_audio_bytes || final_granule != analysis.final_granule {
        return Err(audio_error(
            "decoded Opus packets do not match OGG stream analysis",
        ));
    }
    m4a::decode_to_end(ctx, path, "0:a:0", "decode Opus stream", None)?;
    if analysis.final_granule <= head.pre_skip || analysis.final_granule > decoded_samples {
        return Err(audio_error("opus granule position is invalid"));
    }
    let playable = analysis.final_granule - head.pre_skip;
    let duration_ms = (playable * 1000 / OPUS_SAMPLE_RATE_HZ) as i64;
    let bitrate_kbps = average_bitrate_kbps(analysis.encoded_audio_bytes, duration_ms);
    if duration_ms <= 0 || bitrate_kbps <= 0 {
        return Err(audio_error("opus duration or bitrate is not positive"));
    }
    ctx.report(playable, playable, size_bytes, size_bytes, true)?;
    Ok(TechnicalAudio {
        format: "opus".into(),
        container: "ogg".into(),
        codec: "opus".into(),
        duration_ms,
        sample_rate_hz: OPUS_SAMPLE_RATE_HZ as i64,
        channel_count: i64::from(head.channels),
        bit_depth: 0,
        bitrate_kbps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_ogg_polynomial() {
        assert_eq!(crc(0, b""), 0);
        // CRC-32/MPEG-2 without the final inversion, as Ogg uses it.
        assert_eq!(crc(0, b"123456789"), 0x89a1_897f);
    }

    #[test]
    fn counts_opus_samples_from_the_toc() {
        assert_eq!(opus_packet_samples(&[0xfc]), Some(960)); // CELT 20 ms
        assert_eq!(opus_packet_samples(&[0xfd]), Some(1920)); // two frames
        assert_eq!(opus_packet_samples(&[0x1b, 0x02]), Some(5760)); // SILK 60 ms x2
        assert_eq!(opus_packet_samples(&[0xff, 0x00]), None);
        assert_eq!(opus_packet_samples(&[]), None);
    }

    #[test]
    fn comment_tags_need_a_key() {
        assert!(collect_comment_tags(&["TITLE=x".into()]).is_ok());
        let failure = collect_comment_tags(&["=x".into()]).unwrap_err();
        assert_eq!(failure.issues()[0].reason, "vorbis comment is malformed");
    }

    #[test]
    fn lenient_comments_keep_what_was_read() {
        let mut header = Vec::new();
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&2u32.to_le_bytes());
        header.extend_from_slice(&3u32.to_le_bytes());
        header.extend_from_slice(b"A=b");
        assert_eq!(lenient_vorbis_comments(&header), ["A=b", ""]);
    }
}
