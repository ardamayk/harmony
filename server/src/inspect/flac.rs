//! FLAC (media_inspector.go inspectOpenFLAC). Metadata blocks are parsed
//! here as strictly as mewkiz/flac does; symphonia decodes every frame and
//! checks the decoded audio against the STREAMINFO MD5.

use super::errors::{Code, Failure, Result, error, io_message, optional_artwork};
use super::metadata::{self, Tags, tag_key, tag_string};
use super::{
    AlbumArtwork, Context, CountingFile, MediaInspection, TechnicalAudio, artwork, audio_error,
    hash_file,
};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::default::codecs::FlacDecoder;
use symphonia::default::formats::FlacReader;

const STREAM_INFO_SIZE_BYTES: i64 = 34;
pub const PICTURE_TYPE_FRONT_COVER: u32 = 3;

pub struct Picture {
    pub picture_type: u32,
    pub mime_type: String,
    pub data: Vec<u8>,
}

struct StreamInfo {
    sample_rate: u32,
    channels: u32,
    bits_per_sample: u32,
    total_samples: u64,
}

enum Body {
    Comments(Vec<(String, String)>),
    Picture(Picture),
    Other,
}

struct Block {
    length: i64,
    body: Body,
}

struct Stream {
    info: StreamInfo,
    blocks: Vec<Block>,
}

pub fn inspect(file: &mut File, ctx: &mut Context<'_>) -> Result<MediaInspection> {
    let (file_sha256, size_bytes) = hash_file(file, ctx)?;
    let unsupported = |message: String| error(Code::UnsupportedFormat, "container", message);
    let stream = parse(file).map_err(unsupported)?;
    let tags = collect_tags(&stream.blocks);
    let metadata = metadata::normalize(&tags, metadata::replay_gain_from_tags(&tags));
    let artwork = optional_artwork(front_cover(&stream.blocks));
    let audio = decode(file, &stream, size_bytes, ctx);
    Failure::join([
        metadata.as_ref().err().cloned(),
        artwork.as_ref().err().cloned(),
        audio.as_ref().err().cloned(),
    ])
    .into_result()?;
    Ok(MediaInspection {
        metadata: metadata?,
        album_artwork: artwork?,
        audio: audio?,
        file_sha256,
    })
}

fn read_exact(reader: &mut impl Read, buffer: &mut [u8]) -> std::result::Result<(), String> {
    reader
        .read_exact(buffer)
        .map_err(|cause| io_message(&cause))
}

fn u32_be(data: &[u8]) -> u32 {
    u32::from_be_bytes(data[..4].try_into().unwrap())
}

fn u32_le(data: &[u8]) -> u32 {
    u32::from_le_bytes(data[..4].try_into().unwrap())
}

/// `flac.Parse`: the signature, STREAMINFO first, then every metadata block.
fn parse(file: &mut File) -> std::result::Result<Stream, String> {
    let mut reader = BufReader::new(&mut *file);
    let mut signature = [0u8; 4];
    read_exact(&mut reader, &mut signature)?;
    if &signature != b"fLaC" {
        return Err("FLAC signature is missing".to_owned());
    }
    let (is_last, block_type, body) = read_block(&mut reader)?;
    if block_type != 0 {
        return Err(
            "flac.parseStreamInfo: incorrect type of first metadata block; expected *meta.StreamInfo"
                .to_owned(),
        );
    }
    let info = parse_stream_info(&body)?;
    let mut blocks = Vec::new();
    let mut is_last = is_last;
    while !is_last {
        let (last, block_type, body) = read_block(&mut reader)?;
        is_last = last;
        let length = body.len() as i64;
        let body = match block_type {
            0 => return Err("meta.Block.Parse: duplicate StreamInfo block".to_owned()),
            1 => {
                if body.iter().any(|byte| *byte != 0) {
                    return Err("invalid padding".to_owned());
                }
                Body::Other
            }
            3 => {
                verify_seek_table(&body)?;
                Body::Other
            }
            4 => Body::Comments(parse_comments(&body)?),
            6 => Body::Picture(parse_picture(&body)?),
            127 => return Err("meta.Block.Parse: invalid block type".to_owned()),
            _ => Body::Other,
        };
        blocks.push(Block { length, body });
    }
    let position = reader
        .stream_position()
        .map_err(|cause| io_message(&cause))?;
    drop(reader);
    file.seek(SeekFrom::Start(position))
        .map_err(|cause| io_message(&cause))?;
    Ok(Stream { info, blocks })
}

fn read_block(reader: &mut impl Read) -> std::result::Result<(bool, u8, Vec<u8>), String> {
    let mut header = [0u8; 4];
    read_exact(reader, &mut header)?;
    let length = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
    let mut body = vec![0u8; length];
    read_exact(reader, &mut body)?;
    Ok((header[0] & 0x80 != 0, header[0] & 0x7f, body))
}

fn parse_stream_info(body: &[u8]) -> std::result::Result<StreamInfo, String> {
    if body.len() < STREAM_INFO_SIZE_BYTES as usize {
        return Err("unexpected EOF".to_owned());
    }
    let min_block = u16::from_be_bytes([body[0], body[1]]);
    if min_block < 16 {
        return Err(format!(
            "meta.Block.parseStreamInfo: invalid minimum block size ({min_block}); expected >= 16"
        ));
    }
    let max_block = u16::from_be_bytes([body[2], body[3]]);
    if max_block < 16 {
        return Err(format!(
            "meta.Block.parseStreamInfo: invalid maximum block size ({max_block}); expected >= 16"
        ));
    }
    let packed = u64::from_be_bytes(body[10..18].try_into().unwrap());
    let sample_rate = (packed >> 44) as u32;
    if sample_rate == 0 {
        return Err("meta.Block.parseStreamInfo: invalid sample rate (0)".to_owned());
    }
    Ok(StreamInfo {
        sample_rate,
        channels: ((packed >> 41) & 0x7) as u32 + 1,
        bits_per_sample: ((packed >> 36) & 0x1f) as u32 + 1,
        total_samples: packed & 0xf_ffff_ffff,
    })
}

fn verify_seek_table(body: &[u8]) -> std::result::Result<(), String> {
    const PLACEHOLDER: u64 = u64::MAX;
    let points = body.len() / 18;
    if points < 1 {
        return Err("meta.Block.parseSeekTable: at least one seek point is required".to_owned());
    }
    // mewkiz/flac compares every point with a `prev` it never advances, so
    // only a repeated zero sample number is rejected; keep that behaviour.
    let previous = 0u64;
    for index in 1..points {
        let sample = u64::from_be_bytes(body[index * 18..index * 18 + 8].try_into().unwrap());
        if sample != PLACEHOLDER && sample == previous {
            return Err(format!(
                "meta.Block.parseSeekTable: duplicate seek point with sample number ({sample})"
            ));
        }
    }
    Ok(())
}

/// Reads a length-prefixed field of `size` bytes from `data` at `offset`.
fn take<'a>(
    data: &'a [u8],
    offset: &mut usize,
    size: usize,
) -> std::result::Result<&'a [u8], String> {
    if data.len() - *offset < size {
        return Err("unexpected EOF".to_owned());
    }
    let field = &data[*offset..*offset + size];
    *offset += size;
    Ok(field)
}

fn parse_comments(body: &[u8]) -> std::result::Result<Vec<(String, String)>, String> {
    let mut offset = 0;
    let vendor_length = u32_le(take(body, &mut offset, 4)?) as usize;
    take(body, &mut offset, vendor_length)?;
    let count = u32_le(take(body, &mut offset, 4)?) as usize;
    let mut comments = Vec::new();
    for _ in 0..count {
        let length = u32_le(take(body, &mut offset, 4)?) as usize;
        let vector = take(body, &mut offset, length)?;
        let Some(separator) = vector.iter().position(|byte| *byte == b'=') else {
            return Err(format!(
                "meta.Block.parseVorbisComment: unable to locate '=' in vector {:?}",
                String::from_utf8_lossy(vector)
            ));
        };
        comments.push((
            tag_string(&vector[..separator]),
            tag_string(&vector[separator + 1..]),
        ));
    }
    Ok(comments)
}

/// A FLAC PICTURE block body; Ogg picture comments carry the same layout.
pub fn parse_picture(body: &[u8]) -> std::result::Result<Picture, String> {
    let mut offset = 0;
    let picture_type = u32_be(take(body, &mut offset, 4)?);
    let mime_length = u32_be(take(body, &mut offset, 4)?) as usize;
    let mime_type = tag_string(take(body, &mut offset, mime_length)?);
    let description_length = u32_be(take(body, &mut offset, 4)?) as usize;
    take(body, &mut offset, description_length)?;
    take(body, &mut offset, 16)?;
    let data_length = u32_be(take(body, &mut offset, 4)?) as usize;
    let data = take(body, &mut offset, data_length)?.to_vec();
    Ok(Picture {
        picture_type,
        mime_type,
        data,
    })
}

fn collect_tags(blocks: &[Block]) -> Tags {
    let mut tags = Tags::new();
    for block in blocks {
        if let Body::Comments(comments) = &block.body {
            for (key, value) in comments {
                tags.entry(tag_key(key)).or_default().push(value.clone());
            }
        }
    }
    tags
}

fn front_cover(blocks: &[Block]) -> Result<AlbumArtwork> {
    let mut cover: Option<&Picture> = None;
    for block in blocks {
        let Body::Picture(picture) = &block.body else {
            continue;
        };
        if picture.picture_type == PICTURE_TYPE_FRONT_COVER {
            if cover.is_some() {
                return Err(error(
                    Code::InvalidArtwork,
                    "artwork",
                    "multiple front covers are ambiguous",
                ));
            }
            cover = Some(picture);
        }
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

fn decode(
    file: &File,
    stream: &Stream,
    size_bytes: i64,
    ctx: &mut Context<'_>,
) -> Result<TechnicalAudio> {
    let read = Arc::new(AtomicI64::new(0));
    let mut source = file
        .try_clone()
        .map_err(|cause| audio_error(io_message(&cause)))?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|cause| audio_error(io_message(&cause)))?;
    let source = CountingFile {
        file: source,
        read: read.clone(),
    };
    let stream_source =
        MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
    let mut reader =
        FlacReader::try_new(stream_source, FormatOptions::default()).map_err(audio_error)?;
    let params = reader
        .first_track(TrackType::Audio)
        .and_then(|track| track.codec_params.as_ref())
        .and_then(|params| params.audio())
        .cloned()
        .ok_or_else(|| audio_error("FLAC stream has no audio track"))?;
    let mut options = AudioDecoderOptions::default();
    options.gapless = false;
    // Encoders may leave STREAMINFO's MD5 empty; asking to verify such a stream
    // only makes the decoder warn once per file.
    options.verify = params.verification_check.is_some();
    let mut decoder = FlacDecoder::try_new(&params, &options).map_err(audio_error)?;
    let info = &stream.info;
    let mut decoded_samples = 0u64;
    loop {
        ctx.check()?;
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(cause) => return Err(audio_error(cause)),
        };
        let frames = decoder.decode(&packet).map_err(audio_error)?.frames();
        if frames == 0 {
            return Err(audio_error("decoded frame contains no samples"));
        }
        decoded_samples += frames as u64;
        ctx.report(
            decoded_samples,
            info.total_samples,
            read.load(Ordering::Relaxed),
            size_bytes,
            false,
        )?;
    }
    let verified = decoder.finalize().verify_ok;
    let audio = build_audio(stream, verified, decoded_samples, size_bytes)?;
    ctx.report(
        decoded_samples,
        info.total_samples,
        read.load(Ordering::Relaxed),
        size_bytes,
        true,
    )?;
    Ok(audio)
}

fn build_audio(
    stream: &Stream,
    md5_matches: Option<bool>,
    decoded_samples: u64,
    size_bytes: i64,
) -> Result<TechnicalAudio> {
    let info = &stream.info;
    if decoded_samples == 0 {
        return Err(audio_error(
            "decoded stream has invalid technical properties",
        ));
    }
    if info.total_samples != 0 && decoded_samples != info.total_samples {
        return Err(audio_error(
            "decoded sample count does not match STREAMINFO",
        ));
    }
    if md5_matches == Some(false) {
        return Err(audio_error("decoded audio MD5 does not match STREAMINFO"));
    }
    let duration_ms = (decoded_samples * 1000 / u64::from(info.sample_rate)) as i64;
    if duration_ms <= 0 {
        return Err(audio_error("decoded duration is not positive"));
    }
    let metadata_bytes = 4
        + 4
        + STREAM_INFO_SIZE_BYTES
        + stream
            .blocks
            .iter()
            .map(|block| 4 + block.length)
            .sum::<i64>();
    let audio_bytes = size_bytes - metadata_bytes;
    if audio_bytes <= 0 {
        return Err(audio_error("encoded audio size is not positive"));
    }
    let bitrate_kbps = super::average_bitrate_kbps(audio_bytes, duration_ms);
    if bitrate_kbps <= 0 {
        return Err(audio_error("average encoded bitrate is not positive"));
    }
    Ok(TechnicalAudio {
        format: "flac".into(),
        container: "flac".into(),
        codec: "flac".into(),
        duration_ms,
        sample_rate_hz: i64::from(info.sample_rate),
        channel_count: i64::from(info.channels),
        bit_depth: i64::from(info.bits_per_sample),
        bitrate_kbps,
    })
}
