//! Strict ID3v2 parsing shared by MP3 and WAV (mp3_inspector.go,
//! id3v2_parser.go).

use super::errors::{Code, Failure, Result, error, io_message, optional_artwork};
use super::metadata::{self, NormalizedMetadata, ReplayGain, Tags, go_upper};
use super::{AlbumArtwork, artwork};
use std::collections::HashMap;
use std::io::Read;

pub const HEADER_SIZE_BYTES: usize = 10;
pub const MAX_TAG_SIZE_BYTES: usize = 32 * 1024 * 1024;
const FRONT_COVER_TYPE: u8 = 3;
const OTHER_PICTURE_TYPE: u8 = 0;
const LYRICS_TEXT_OFFSET: usize = 4;

const LATIN1: u8 = 0;
const UTF16: u8 = 1;
const UTF16BE: u8 = 2;
const UTF8: u8 = 3;

pub struct Frame {
    name: String,
    payload: Vec<u8>,
}

#[derive(Default)]
pub struct Values {
    pub tags: Tags,
    replay_gain: HashMap<String, String>,
    pub pictures: Vec<TaggedPicture>,
}

pub struct TaggedPicture {
    mime_type: String,
    data: Vec<u8>,
    picture_type: u8,
}

/// `invalidID3Error`.
pub fn invalid(message: impl std::fmt::Display) -> Failure {
    error(Code::InvalidMetadata, "ID3", message)
}

/// The parsed tag of an MP3 file; `offset` is where the audio starts.
pub struct Tag {
    pub metadata: Result<NormalizedMetadata>,
    pub artwork: AlbumArtwork,
    pub offset: u64,
    /// Failures of the tag that do not stop the audio checks.
    pub failure: Failure,
}

/// `inspectID3Tag`: `Err` when the tag structure itself cannot be read.
pub fn inspect_tag(reader: &mut impl Read) -> Result<Tag> {
    let (frames, version, offset) = read_frames(reader)?;
    let (values, parse_failure) = collect_values(&frames, version);
    let failed_fields = parse_failure.fields();
    let metadata = normalize(&values);
    let mut artwork = optional_artwork(
        select_front_cover(&values.pictures)
            .and_then(|picture| artwork::validate(&picture.mime_type, &picture.data)),
    );
    let mut parse_failure = parse_failure;
    if let Some(issue) = parse_failure
        .issues()
        .into_iter()
        .find(|issue| issue.field == "artwork")
    {
        artwork = optional_artwork(Err(error(Code::InvalidArtwork, "artwork", issue.reason)));
        parse_failure = parse_failure.exclude_fields(&["artwork".to_owned()]);
    }
    let rest = Failure::join([
        metadata.as_ref().err().cloned(),
        artwork.as_ref().err().cloned(),
    ])
    .exclude_fields(&failed_fields);
    Ok(Tag {
        metadata,
        artwork: artwork.unwrap_or_default(),
        offset,
        failure: Failure::join([Some(parse_failure), Some(rest)]),
    })
}

fn read_frames(reader: &mut impl Read) -> Result<(Vec<Frame>, u8, u64)> {
    let mut header = [0u8; HEADER_SIZE_BYTES];
    reader
        .read_exact(&mut header)
        .map_err(|cause| invalid(format!("read ID3 header: {}", io_message(&cause))))?;
    let (version, size) = parse_header(&header)?;
    let mut payload = vec![0u8; size];
    reader
        .read_exact(&mut payload)
        .map_err(|cause| invalid(format!("read ID3 payload: {}", io_message(&cause))))?;
    let frames = parse_frames(&payload, version)?;
    Ok((frames, version, (HEADER_SIZE_BYTES + size) as u64))
}

/// `parseID3Header`: the version and payload size.
pub fn parse_header(header: &[u8]) -> Result<(u8, usize)> {
    if header.len() < HEADER_SIZE_BYTES || &header[..3] != b"ID3" {
        return Err(invalid("ID3v2 tag is required"));
    }
    let (version, revision) = (header[3], header[4]);
    if !(2..=4).contains(&version) || revision != 0 {
        return Err(invalid(format!(
            "unsupported ID3v2 version {version}.{revision}"
        )));
    }
    if header[5] != 0 {
        return Err(invalid(
            "ID3 tag flags are not supported by the Strict Import Profile",
        ));
    }
    match decode_sync_safe(&header[6..10]) {
        Some(size) if size > 0 && size <= MAX_TAG_SIZE_BYTES => Ok((version, size)),
        _ => Err(invalid("ID3 tag size is invalid")),
    }
}

fn decode_sync_safe(data: &[u8]) -> Option<usize> {
    if data.len() != 4 {
        return None;
    }
    let mut value = 0usize;
    for part in data {
        if part & 0x80 != 0 {
            return None;
        }
        value = value << 7 | usize::from(part & 0x7f);
    }
    Some(value)
}

fn is_zero(data: &[u8]) -> bool {
    data.iter().all(|byte| *byte == 0)
}

/// `parseID3Frames`.
pub fn parse_frames(payload: &[u8], version: u8) -> Result<Vec<Frame>> {
    let (name_size, header_size) = if version == 2 { (3, 6) } else { (4, 10) };
    let mut frames = Vec::new();
    let mut offset = 0;
    while offset < payload.len() {
        if payload.len() - offset < header_size {
            if is_zero(&payload[offset..]) {
                break;
            }
            return Err(invalid("ID3 frame header is truncated"));
        }
        if is_zero(&payload[offset..offset + name_size]) {
            if !is_zero(&payload[offset..]) {
                return Err(invalid("ID3 padding contains non-zero data"));
            }
            break;
        }
        let size = match parse_frame_header(&payload[offset..], version) {
            Some(size) if size > 0 && size <= payload.len() - offset - header_size => size,
            _ => return Err(invalid("ID3 frame size is invalid")),
        };
        let name = String::from_utf8_lossy(&payload[offset..offset + name_size]).into_owned();
        let start = offset + header_size;
        frames.push(Frame {
            name,
            payload: payload[start..start + size].to_vec(),
        });
        offset = start + size;
    }
    Ok(frames)
}

fn parse_frame_header(data: &[u8], version: u8) -> Option<usize> {
    let name_size = if version == 2 { 3 } else { 4 };
    if !data[..name_size]
        .iter()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return None;
    }
    if version == 2 {
        return Some(
            data[3..6]
                .iter()
                .fold(0, |value, byte| value << 8 | usize::from(*byte)),
        );
    }
    if !is_zero(&data[8..10]) {
        return None;
    }
    if version == 4 {
        return decode_sync_safe(&data[4..8]);
    }
    Some(u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize)
}

/// `collectID3Values`: every frame, with each frame's failure in order.
pub fn collect_values(frames: &[Frame], version: u8) -> (Values, Failure) {
    let mut values = Values::default();
    let failures = frames
        .iter()
        .map(|frame| collect_frame(&mut values, frame, version).err())
        .collect::<Vec<_>>();
    (values, Failure::join(failures))
}

fn canonical_name(name: &str, version: u8) -> Option<&'static str> {
    Some(match (version, name) {
        (2, "TT2") | (3 | 4, "TIT2") => "TITLE",
        (2, "TP1") | (3 | 4, "TPE1") => "ARTIST",
        (2, "TP2") | (3 | 4, "TPE2") => "ALBUMARTIST",
        (2, "TAL") | (3 | 4, "TALB") => "ALBUM",
        (2, "TRK") | (3 | 4, "TRCK") => "TRACKNUMBER",
        (2, "TPA") | (3 | 4, "TPOS") => "DISCNUMBER",
        (2, "TCO") | (3 | 4, "TCON") => "GENRE",
        (2, "TYE") | (3, "TYER") | (4, "TDRC") => "DATE",
        _ => return None,
    })
}

fn collect_frame(values: &mut Values, frame: &Frame, version: u8) -> Result<()> {
    let v2 = version == 2;
    if let Some(key) = canonical_name(&frame.name, version) {
        if (key == "GENRE" || key == "DATE") && frame.payload.len() == 1 && frame.payload[0] <= UTF8
        {
            return Ok(());
        }
        let mut frame_values = decode_text_values(&frame.payload, version)
            .map_err(|cause| error(Code::InvalidMetadata, key, cause))?;
        if key == "GENRE" {
            frame_values = metadata::split_genre_tag_values(&frame_values);
        }
        values
            .tags
            .entry(key.to_owned())
            .or_default()
            .extend(frame_values);
        return Ok(());
    }
    if frame.name == if v2 { "TXX" } else { "TXXX" } {
        collect_user_text(values, &frame.payload).map_err(invalid)?;
    }
    if frame.name == if v2 { "PIC" } else { "APIC" } {
        let (mime_type, data, picture_type) = decode_picture(&frame.payload, version)
            .map_err(|cause| error(Code::InvalidArtwork, "artwork", cause))?;
        values.pictures.push(TaggedPicture {
            mime_type,
            data,
            picture_type,
        });
        return Ok(());
    }
    if frame.name == if v2 { "ULT" } else { "USLT" } {
        return collect_lyrics(values, &frame.payload);
    }
    Ok(())
}

/// An unsynchronised lyrics frame: encoding, a three-letter language, a
/// terminated content descriptor, then the text.
fn collect_lyrics(values: &mut Values, payload: &[u8]) -> Result<()> {
    if payload.len() < LYRICS_TEXT_OFFSET {
        return Err(invalid("lyrics frame is truncated"));
    }
    let encoding = payload[0];
    let (_, remainder) =
        split_encoded_field(encoding, &payload[LYRICS_TEXT_OFFSET..]).map_err(invalid)?;
    let text = decode_text(encoding, remainder).map_err(invalid)?;
    values
        .tags
        .entry("LYRICS".to_owned())
        .or_default()
        .push(text);
    Ok(())
}

fn collect_user_text(values: &mut Values, payload: &[u8]) -> std::result::Result<(), String> {
    if payload.len() < 2 {
        return Err("described text frame is truncated".to_owned());
    }
    let encoding = payload[0];
    let (description, remainder) = split_encoded_field(encoding, &payload[1..])?;
    let value = decode_text(encoding, remainder)?;
    let key = go_upper(description.trim());
    if key == "ARTISTS" || key == "ALBUMARTISTS" {
        // Custom credits use NUL-separated values even before ID3v2.4; one
        // final NUL terminates the text.
        let credits = value.strip_suffix('\0').unwrap_or(&value);
        let entry = values.tags.entry(key.clone()).or_default();
        for credit in credits.split('\0') {
            let credit = if encoding == UTF16 {
                credit.strip_prefix('\u{feff}').unwrap_or(credit)
            } else {
                credit
            };
            entry.push(credit.to_owned());
        }
    }
    if key.starts_with("REPLAYGAIN_") {
        if values.replay_gain.contains_key(&key) {
            return Err(format!("duplicate {key} tag"));
        }
        values.replay_gain.insert(key, value);
    }
    Ok(())
}

pub fn select_front_cover(pictures: &[TaggedPicture]) -> Result<&TaggedPicture> {
    let front: Vec<&TaggedPicture> = pictures
        .iter()
        .filter(|picture| picture.picture_type == FRONT_COVER_TYPE)
        .collect();
    let invalid_artwork = |message: &str| error(Code::InvalidArtwork, "artwork", message);
    if front.len() == 1 {
        return Ok(front[0]);
    }
    if front.len() > 1 || pictures.len() > 1 {
        return Err(invalid_artwork("embedded front cover role is ambiguous"));
    }
    match pictures.first() {
        Some(picture) if picture.picture_type == OTHER_PICTURE_TYPE => Ok(picture),
        Some(_) => Err(invalid_artwork("embedded picture is not a front cover")),
        None => Err(error(
            Code::MissingArtwork,
            "artwork",
            "embedded front cover is required",
        )),
    }
}

/// `inspectWAVArtwork` / `inspectID3Artwork`.
pub fn front_cover_artwork(pictures: &[TaggedPicture]) -> Result<AlbumArtwork> {
    let picture = select_front_cover(pictures)?;
    artwork::validate(&picture.mime_type, &picture.data)
}

fn decode_picture(
    payload: &[u8],
    version: u8,
) -> std::result::Result<(String, Vec<u8>, u8), String> {
    if payload.len() < 6 {
        return Err("embedded picture frame is truncated".to_owned());
    }
    let encoding = payload[0];
    let mut offset = 1;
    let mime_type = if version == 2 {
        let format = go_upper(&String::from_utf8_lossy(&payload[1..4]));
        offset += 3;
        match format.as_str() {
            "JPG" | "JPEG" => "image/jpeg",
            "PNG" => "image/png",
            _ => "",
        }
        .to_owned()
    } else {
        let Some(end) = payload[offset..].iter().position(|byte| *byte == 0) else {
            return Err("embedded picture MIME type is truncated".to_owned());
        };
        let mime_type = metadata::tag_string(&payload[offset..offset + end]);
        offset += end + 1;
        mime_type
    };
    if offset >= payload.len() {
        return Err("embedded picture type is truncated".to_owned());
    }
    let picture_type = payload[offset];
    offset += 1;
    match split_encoded_field(encoding, &payload[offset..]) {
        Ok((_, data)) if !data.is_empty() => Ok((mime_type, data.to_vec(), picture_type)),
        _ => Err("embedded picture description or data is truncated".to_owned()),
    }
}

fn decode_text_values(payload: &[u8], version: u8) -> std::result::Result<Vec<String>, String> {
    if payload.len() < 2 {
        return Err("text frame is empty".to_owned());
    }
    let value = decode_text(payload[0], &payload[1..])?;
    if version != 4 {
        return Ok(vec![value]);
    }
    Ok(value.split('\0').map(str::to_owned).collect())
}

/// Splits a terminated text field from the bytes that follow it.
fn split_encoded_field(encoding: u8, data: &[u8]) -> std::result::Result<(String, &[u8]), String> {
    let wide = encoding == UTF16 || encoding == UTF16BE;
    let separator = if wide {
        (0..data.len().saturating_sub(1))
            .step_by(2)
            .find(|offset| data[*offset] == 0 && data[offset + 1] == 0)
    } else {
        data.iter().position(|byte| *byte == 0)
    };
    let Some(separator) = separator else {
        return Err("encoded text separator is missing".to_owned());
    };
    let value = decode_text(encoding, &data[..separator])?;
    Ok((value, &data[separator + if wide { 2 } else { 1 }..]))
}

fn decode_text(encoding: u8, data: &[u8]) -> std::result::Result<String, String> {
    match encoding {
        LATIN1 => Ok(data.iter().map(|byte| char::from(*byte)).collect()),
        UTF16 => decode_utf16(data, true),
        UTF16BE => decode_utf16(data, false),
        UTF8 => std::str::from_utf8(data)
            .map(str::to_owned)
            .map_err(|_| "ID3 text is not valid UTF-8".to_owned()),
        other => Err(format!("unsupported ID3 text encoding {other}")),
    }
}

fn decode_utf16(mut data: &[u8], has_bom: bool) -> std::result::Result<String, String> {
    let mut little_endian = false;
    if has_bom {
        if data.len() < 2 {
            return Err("UTF-16 byte order mark is missing".to_owned());
        }
        match (data[0], data[1]) {
            (0xff, 0xfe) => little_endian = true,
            (0xfe, 0xff) => {}
            _ => return Err("UTF-16 byte order mark is invalid".to_owned()),
        }
        data = &data[2..];
    }
    if !data.len().is_multiple_of(2) {
        return Err("UTF-16 text has an odd byte count".to_owned());
    }
    let units = data.as_chunks::<2>().0.iter().map(|&pair| {
        if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    });
    Ok(char::decode_utf16(units)
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect())
}

/// `normalizeID3Metadata`: ReplayGain values from TXXX frames must parse.
fn normalize(values: &Values) -> Result<NormalizedMetadata> {
    let parse = |key: &str, peak: bool| -> Result<Option<f64>> {
        let Some(value) = values.replay_gain.get(key) else {
            return Ok(None);
        };
        let parsed = if peak {
            metadata::parse_replay_gain_peak(value)
        } else {
            metadata::parse_replay_gain_value(value)
        };
        parsed
            .map(Some)
            .ok_or_else(|| error(Code::InvalidMetadata, key, "ReplayGain value is invalid"))
    };
    let track_gain = parse("REPLAYGAIN_TRACK_GAIN", false);
    let track_peak = parse("REPLAYGAIN_TRACK_PEAK", true);
    let album_gain = parse("REPLAYGAIN_ALBUM_GAIN", false);
    let album_peak = parse("REPLAYGAIN_ALBUM_PEAK", true);
    let replay_gain = ReplayGain {
        track_gain_db: track_gain.clone().unwrap_or_default(),
        track_peak: track_peak.clone().unwrap_or_default(),
        album_gain_db: album_gain.clone().unwrap_or_default(),
        album_peak: album_peak.clone().unwrap_or_default(),
    };
    let normalized = metadata::normalize(&values.tags, replay_gain);
    Failure::join([
        track_gain.err(),
        track_peak.err(),
        album_gain.err(),
        album_peak.err(),
        normalized.as_ref().err().cloned(),
    ])
    .into_result()?;
    normalized
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An ID3v2.3 tag with the given text frames and pictures.
    pub fn tag(frames: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut payload = Vec::new();
        for (name, body) in frames {
            payload.extend_from_slice(name.as_bytes());
            payload.extend_from_slice(&(body.len() as u32).to_be_bytes());
            payload.extend_from_slice(&[0, 0]);
            payload.extend_from_slice(body);
        }
        let size = payload.len();
        let mut out = b"ID3\x03\x00\x00".to_vec();
        out.extend_from_slice(&[
            (size >> 21) as u8 & 0x7f,
            (size >> 14) as u8 & 0x7f,
            (size >> 7) as u8 & 0x7f,
            size as u8 & 0x7f,
        ]);
        out.extend_from_slice(&payload);
        out
    }

    pub fn text(value: &str) -> Vec<u8> {
        let mut body = vec![UTF8];
        body.extend_from_slice(value.as_bytes());
        body
    }

    pub fn user_text(description: &str, value: &str) -> Vec<u8> {
        let mut body = vec![UTF8];
        body.extend_from_slice(description.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        body
    }

    pub fn identity() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("TIT2", text("Song")),
            ("TPE1", text("Artist")),
            ("TPE2", text("Album Artist")),
            ("TALB", text("Album")),
            ("TRCK", text("1/2")),
            ("TCON", text("Pop; Rock")),
        ]
    }

    #[test]
    fn reads_identity_credits_and_replay_gain() {
        let mut frames = identity();
        frames.push(("TXXX", user_text("ARTISTS", "One\0Two\0")));
        frames.push(("TXXX", user_text("replaygain_track_gain", "-3.5 dB")));
        let tag = inspect_tag(&mut &tag(&frames)[..]).unwrap();
        assert!(tag.failure.is_empty(), "{}", tag.failure);
        let metadata = tag.metadata.unwrap();
        assert_eq!(metadata.artists, ["One", "Two"]);
        assert_eq!(metadata.genres, ["Pop", "Rock"]);
        assert_eq!(metadata.replay_gain.track_gain_db, Some(-3.5));
        assert_eq!(tag.artwork, AlbumArtwork::default());
    }

    #[test]
    fn bad_pictures_become_a_warning_and_bad_text_fails_its_field() {
        let mut frames = identity();
        frames[0] = ("TIT2", vec![9, b'x']);
        frames.push(("APIC", vec![0, 1, 2]));
        let tag = inspect_tag(&mut &tag(&frames)[..]).unwrap();
        assert_eq!(
            tag.artwork.warning,
            crate::inspect::errors::INVALID_ARTWORK_WARNING
        );
        let issues: Vec<_> = tag
            .failure
            .issues()
            .into_iter()
            .map(|i| (i.field, i.reason))
            .collect();
        assert_eq!(
            issues,
            [(
                "TITLE".to_owned(),
                "unsupported ID3 text encoding 9".to_owned()
            )]
        );
    }

    #[test]
    fn rejects_bad_headers_and_frames() {
        let reason = |data: &[u8]| {
            inspect_tag(&mut &data[..]).err().unwrap().issues()[0]
                .reason
                .clone()
        };
        assert_eq!(
            reason(b"ID3\x05\x00\x00\x00\x00\x00\x01"),
            "unsupported ID3v2 version 5.0"
        );
        assert_eq!(
            reason(b"ID3\x03\x00\x40\x00\x00\x00\x01"),
            "ID3 tag flags are not supported by the Strict Import Profile"
        );
        assert_eq!(
            reason(b"ID3\x03\x00\x00\x00\x00\x00\x20"),
            "read ID3 payload"
        );
        let mut bad = tag(&identity());
        bad[10] = b'a';
        assert_eq!(reason(&bad), "ID3 frame size is invalid");
    }

    #[test]
    fn decodes_utf16_with_and_without_bom() {
        assert_eq!(decode_text(UTF16, b"\xff\xfeA\x00").unwrap(), "A");
        assert_eq!(decode_text(UTF16BE, b"\x00A").unwrap(), "A");
        assert_eq!(
            decode_text(UTF16, b"A\x00").unwrap_err(),
            "UTF-16 byte order mark is invalid"
        );
        assert_eq!(decode_text(LATIN1, b"\xe9").unwrap(), "\u{e9}");
    }
}
