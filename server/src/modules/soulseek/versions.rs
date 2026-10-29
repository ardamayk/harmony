//! Acquisition Versions: slskd's search responses grouped by the quality
//! their files report. A pure function, so the rules are tested without
//! HTTP. Nothing is inferred: an attribute a file does not report forms its
//! own "unknown" version, and the order is for display, never a ranking of
//! one quality over another.

use super::slskd::{SearchFile, SearchResponse};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::BTreeMap;

/// The audio formats Managed Import accepts, by file extension.
const AUDIO_FORMATS: [&str; 6] = ["flac", "wav", "m4a", "mp3", "ogg", "opus"];
/// Formats that are always lossless. An `m4a` may hold ALAC or AAC; it is
/// treated as lossy, keyed by the bitrate it reports.
const LOSSLESS_FORMATS: [&str; 2] = ["flac", "wav"];
/// Cover images a source folder may hold next to its audio.
const ARTWORK_EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

/// What one acquisition takes: the whole Album, or one Track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// One source is one user's folder.
    Album,
    /// One source is one user's file.
    Track,
}

impl Scope {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "album" => Some(Self::Album),
            "track" => Some(Self::Track),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Album => "album",
            Self::Track => "track",
        }
    }
}

/// LAME's typical average bitrates for its VBR presets V0 to V9 (kbps).
const MP3_VBR_LEVELS: [u32; 10] = [245, 225, 190, 175, 165, 130, 115, 100, 85, 65];
/// Other formats' variable bitrates are rounded to a multiple of this.
const VBR_STEP_KBPS: u32 = 32;

/// The quality a file reports. Lossless files carry bit depth and sample
/// rate; lossy files carry bitrate and whether it is variable. A variable
/// bitrate is an average that differs per file, so a source's level is
/// the mean of its files' bitrates, rounded by [`vbr_level`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quality {
    pub format: String,
    pub lossless: bool,
    pub bit_depth: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub variable_bitrate: Option<bool>,
}

/// The level a variable bitrate is grouped by, so that one encoder setting
/// forms one version while its files' averages differ: an MP3 goes to the
/// nearest LAME preset average (V0 ~245, V2 ~190 kbps; a tie goes to the
/// higher), any other format to the nearest multiple of 32 kbps.
fn vbr_level(format: &str, mean_kbps: u32) -> u32 {
    if format == "mp3" {
        return MP3_VBR_LEVELS
            .into_iter()
            .min_by_key(|level| level.abs_diff(mean_kbps))
            .unwrap_or(mean_kbps);
    }
    ((mean_kbps + VBR_STEP_KBPS / 2) / VBR_STEP_KBPS * VBR_STEP_KBPS).max(VBR_STEP_KBPS)
}

impl Quality {
    /// A file's quality; for a variable bitrate without the bitrate, which
    /// the source's files give together (see [`vbr_level`]).
    fn of(format: &str, file: &SearchFile) -> Self {
        let reported = |value: Option<u32>| value.filter(|value| *value > 0);
        if LOSSLESS_FORMATS.contains(&format) {
            return Self {
                format: format.to_owned(),
                lossless: true,
                bit_depth: reported(file.bit_depth),
                sample_rate_hz: reported(file.sample_rate),
                bitrate_kbps: None,
                variable_bitrate: None,
            };
        }
        let variable_bitrate = file.is_variable_bit_rate;
        Self {
            format: format.to_owned(),
            lossless: false,
            bit_depth: None,
            sample_rate_hz: None,
            bitrate_kbps: if variable_bitrate == Some(true) {
                None
            } else {
                reported(file.bit_rate)
            },
            variable_bitrate,
        }
    }

    /// The version key: stable for the same quality in any search, such as
    /// `flac-24-48000`, `flac-x-x` (unknown), `mp3-320-cbr`, `mp3-vbr-245`
    /// or `mp3-vbr-x` (variable, no bitrate reported).
    pub fn key(&self) -> String {
        let known =
            |value: Option<u32>| value.map_or_else(|| "x".to_owned(), |value| value.to_string());
        if self.lossless {
            return format!(
                "{}-{}-{}",
                self.format,
                known(self.bit_depth),
                known(self.sample_rate_hz)
            );
        }
        match self.variable_bitrate {
            Some(true) => format!("{}-vbr-{}", self.format, known(self.bitrate_kbps)),
            Some(false) => format!("{}-{}-cbr", self.format, known(self.bitrate_kbps)),
            None => format!("{}-{}-x", self.format, known(self.bitrate_kbps)),
        }
    }
}

/// One file of a source, by the remote path slskd downloads it with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteFile {
    pub filename: String,
    pub size: u64,
}

/// One Acquisition Source: a user's folder (Album scope) or file (Track
/// scope) in one quality.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    /// Stable within its version: derived from the user and the folder or
    /// file, so the same search always yields the same id.
    pub id: String,
    pub username: String,
    pub folder: String,
    /// Audio files, ordered by remote path.
    pub files: Vec<RemoteFile>,
    /// Cover images in the same folder, for the Album Artwork.
    pub artwork: Vec<RemoteFile>,
    pub total_bytes: u64,
    /// False for an Album source with fewer files than the largest source
    /// of its version.
    pub complete: bool,
    pub has_free_upload_slot: bool,
    pub queue_length: u64,
    pub upload_speed: u64,
}

/// One Acquisition Version: every source of one quality.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub key: String,
    pub quality: Quality,
    /// The largest file count among the sources.
    pub file_count: usize,
    /// The size of the largest source.
    pub total_bytes: u64,
    pub complete_source_count: usize,
    /// Complete first, then free upload slot, shorter queue, faster speed.
    pub sources: Vec<Source>,
}

/// Groups search responses into versions, ordered by complete-source
/// count, then source count.
pub fn group(scope: Scope, responses: &[SearchResponse]) -> Vec<Version> {
    let mut by_key: BTreeMap<String, (Quality, Vec<Source>)> = BTreeMap::new();
    for (username, user) in by_user(responses) {
        for (quality, source) in sources(scope, &username, &user) {
            by_key
                .entry(quality.key())
                .or_insert_with(|| (quality, Vec::new()))
                .1
                .push(source);
        }
    }
    let mut versions: Vec<Version> = by_key
        .into_iter()
        .map(|(key, (quality, sources))| version(scope, key, quality, sources))
        .collect();
    versions.sort_by_key(|version| {
        (
            Reverse(version.complete_source_count),
            Reverse(version.sources.len()),
            version.key.clone(),
        )
    });
    versions
}

/// One user's files across their responses, without repeats.
struct UserFiles<'a> {
    has_free_upload_slot: bool,
    queue_length: u64,
    upload_speed: u64,
    files: BTreeMap<&'a str, &'a SearchFile>,
}

fn by_user(responses: &[SearchResponse]) -> BTreeMap<String, UserFiles<'_>> {
    let mut users: BTreeMap<String, UserFiles<'_>> = BTreeMap::new();
    for response in responses {
        let user = users
            .entry(response.username.clone())
            .or_insert_with(|| UserFiles {
                has_free_upload_slot: response.has_free_upload_slot,
                queue_length: response.queue_length,
                upload_speed: response.upload_speed,
                files: BTreeMap::new(),
            });
        for file in &response.files {
            user.files.entry(file.filename.as_str()).or_insert(file);
        }
    }
    users
}

/// Files of one source being gathered: their quality without a variable
/// bitrate's level, and the bitrates the files report.
struct Gathered {
    quality: Quality,
    files: Vec<RemoteFile>,
    bitrates: Vec<u32>,
}

/// A user's sources: audio files grouped by folder (or one per file) and
/// quality, with the folder's cover images. A variable bitrate's level is
/// the source's, from the mean of its files' bitrates, so one folder is
/// never split by the bitrate each file happens to average.
fn sources(scope: Scope, username: &str, user: &UserFiles<'_>) -> Vec<(Quality, Source)> {
    let mut artwork: BTreeMap<&str, Vec<RemoteFile>> = BTreeMap::new();
    // (quality key, folder, file for Track scope) -> files
    let mut groups: BTreeMap<(String, &str, &str), Gathered> = BTreeMap::new();
    for (filename, file) in &user.files {
        let (folder, name) = split_path(filename);
        let Some(extension) = extension(name) else {
            continue;
        };
        let remote = RemoteFile {
            filename: (*filename).to_owned(),
            size: file.size,
        };
        if ARTWORK_EXTENSIONS.contains(&extension.as_str()) {
            artwork.entry(folder).or_default().push(remote);
            continue;
        }
        if !AUDIO_FORMATS.contains(&extension.as_str()) {
            continue;
        }
        let quality = Quality::of(&extension, file);
        let item = match scope {
            Scope::Album => "",
            Scope::Track => filename,
        };
        let bitrate = file.bit_rate.filter(|bitrate| *bitrate > 0);
        let gathered = groups
            .entry((quality.key(), folder, item))
            .or_insert_with(|| Gathered {
                quality,
                files: Vec::new(),
                bitrates: Vec::new(),
            });
        gathered.files.push(remote);
        gathered.bitrates.extend(bitrate);
    }
    groups
        .into_iter()
        .map(|((_, folder, item), gathered)| {
            let Gathered {
                mut quality,
                files,
                bitrates,
            } = gathered;
            if quality.variable_bitrate == Some(true) && !bitrates.is_empty() {
                let count = bitrates.len() as u64;
                let sum: u64 = bitrates.iter().copied().map(u64::from).sum();
                let mean = u32::try_from((sum + count / 2) / count).unwrap_or(u32::MAX);
                quality.bitrate_kbps = Some(vbr_level(&quality.format, mean));
            }
            let path = if item.is_empty() { folder } else { item };
            let source = Source {
                id: source_id(username, path),
                username: username.to_owned(),
                folder: folder.to_owned(),
                total_bytes: files.iter().map(|file| file.size).sum(),
                files,
                artwork: artwork.get(folder).cloned().unwrap_or_default(),
                complete: true,
                has_free_upload_slot: user.has_free_upload_slot,
                queue_length: user.queue_length,
                upload_speed: user.upload_speed,
            };
            (quality, source)
        })
        .collect()
}

fn version(scope: Scope, key: String, quality: Quality, mut sources: Vec<Source>) -> Version {
    let (file_count, total_bytes) = sources
        .iter()
        .map(|source| (source.files.len(), source.total_bytes))
        .max()
        .unwrap_or_default();
    if scope == Scope::Album {
        for source in &mut sources {
            source.complete = source.files.len() == file_count;
        }
    }
    sources.sort_by(|a, b| {
        (
            Reverse(a.complete),
            Reverse(a.has_free_upload_slot),
            a.queue_length,
            Reverse(a.upload_speed),
            &a.username,
            &a.folder,
            &a.id,
        )
            .cmp(&(
                Reverse(b.complete),
                Reverse(b.has_free_upload_slot),
                b.queue_length,
                Reverse(b.upload_speed),
                &b.username,
                &b.folder,
                &b.id,
            ))
    });
    Version {
        key,
        quality,
        file_count,
        total_bytes,
        complete_source_count: sources.iter().filter(|source| source.complete).count(),
        sources,
    }
}

/// Splits a remote path at its last separator. Soulseek paths use
/// backslashes; some clients send slashes.
fn split_path(filename: &str) -> (&str, &str) {
    match filename.rfind(['\\', '/']) {
        Some(index) => (&filename[..index], &filename[index + 1..]),
        None => ("", filename),
    }
}

fn extension(name: &str) -> Option<String> {
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then(|| extension.to_ascii_lowercase())
}

/// 16 hex digits of SHA-256 over the user and the folder or file.
fn source_id(username: &str, path: &str) -> String {
    let digest = Sha256::new()
        .chain_update(username.as_bytes())
        .chain_update([0])
        .chain_update(path.as_bytes())
        .finalize();
    hex::encode(&digest[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Responses as slskd records them, trimmed to the fields it reads.
    fn responses(json: &str) -> Vec<SearchResponse> {
        serde_json::from_str(json).expect("fixture")
    }

    fn keys(versions: &[Version]) -> Vec<&str> {
        versions
            .iter()
            .map(|version| version.key.as_str())
            .collect()
    }

    #[test]
    fn only_supported_audio_counts_and_cover_images_are_remembered() {
        let versions = group(
            Scope::Album,
            &responses(
                r#"[{"username": "alice", "hasFreeUploadSlot": true, "queueLength": 0, "uploadSpeed": 10, "files": [
                    {"filename": "@@a\\Album\\01.FLAC", "size": 5, "extension": "", "bitDepth": 16, "sampleRate": 44100, "code": 1},
                    {"filename": "@@a\\Album\\02.flac", "size": 6, "extension": "", "bitDepth": 16, "sampleRate": 44100},
                    {"filename": "@@a\\Album\\Folder.JPG", "size": 1},
                    {"filename": "@@a\\Album\\back.png", "size": 2},
                    {"filename": "@@a\\Album\\album.cue", "size": 1},
                    {"filename": "@@a\\Album\\album.log", "size": 1},
                    {"filename": "@@a\\Album\\video.mkv", "size": 1},
                    {"filename": "@@a\\Other\\cover.jpg", "size": 1},
                    {"filename": "@@a\\Album\\.flac", "size": 1}
                ]}]"#,
            ),
        );
        assert_eq!(keys(&versions), ["flac-16-44100"]);
        let source = &versions[0].sources[0];
        assert_eq!(source.folder, r"@@a\Album");
        assert_eq!(
            source.files,
            [
                RemoteFile {
                    filename: r"@@a\Album\01.FLAC".to_owned(),
                    size: 5
                },
                RemoteFile {
                    filename: r"@@a\Album\02.flac".to_owned(),
                    size: 6
                },
            ]
        );
        assert_eq!(source.total_bytes, 11);
        let artwork: Vec<&str> = source
            .artwork
            .iter()
            .map(|file| file.filename.as_str())
            .collect();
        assert_eq!(artwork, [r"@@a\Album\Folder.JPG", r"@@a\Album\back.png"]);
    }

    #[test]
    fn missing_attributes_form_their_own_version_and_disagreeing_files_split() {
        let versions = group(
            Scope::Album,
            &responses(
                r#"[{"username": "bob", "files": [
                    {"filename": "M\\A\\01.flac", "size": 1, "bitDepth": 24, "sampleRate": 96000},
                    {"filename": "M\\A\\02.flac", "size": 1, "bitDepth": 24},
                    {"filename": "M\\A\\03.flac", "size": 1, "bitDepth": 0, "sampleRate": 0},
                    {"filename": "M\\A\\04.mp3", "size": 1, "bitRate": 320},
                    {"filename": "M\\A\\05.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false},
                    {"filename": "M\\A\\06.mp3", "size": 1, "isVariableBitRate": false},
                    {"filename": "M\\A\\07.mp3", "size": 1, "bitRate": 245, "isVariableBitRate": true},
                    {"filename": "M\\A\\08.mp3", "size": 1, "bitRate": 251, "isVariableBitRate": true},
                    {"filename": "M\\A\\09.wav", "size": 1, "bitDepth": 16, "sampleRate": 44100, "bitRate": 1411}
                ]}]"#,
            ),
        );
        let mut found = keys(&versions);
        found.sort_unstable();
        assert_eq!(
            found,
            [
                "flac-24-96000",
                "flac-24-x",
                "flac-x-x",
                "mp3-320-cbr",
                "mp3-320-x",
                "mp3-vbr-245",
                "mp3-x-cbr",
                "wav-16-44100",
            ]
        );
        let vbr = versions
            .iter()
            .find(|version| version.key == "mp3-vbr-245")
            .unwrap();
        assert_eq!(vbr.sources.len(), 1);
        assert_eq!(vbr.sources[0].files.len(), 2);
        assert_eq!(
            vbr.quality,
            Quality {
                format: "mp3".to_owned(),
                lossless: false,
                bit_depth: None,
                sample_rate_hz: None,
                bitrate_kbps: Some(245),
                variable_bitrate: Some(true),
            }
        );
        let unknown = versions
            .iter()
            .find(|version| version.key == "flac-x-x")
            .unwrap();
        assert_eq!(
            (unknown.quality.bit_depth, unknown.quality.sample_rate_hz),
            (None, None)
        );
    }

    #[test]
    fn variable_bitrates_group_by_the_source_average_level() {
        let versions = group(
            Scope::Album,
            &responses(
                r#"[
                {"username": "v0", "files": [
                    {"filename": "A\\V0\\1.mp3", "size": 1, "bitRate": 228, "isVariableBitRate": true},
                    {"filename": "A\\V0\\2.mp3", "size": 1, "bitRate": 262, "isVariableBitRate": true}]},
                {"username": "v0b", "files": [
                    {"filename": "B\\V0\\1.mp3", "size": 1, "bitRate": 251, "isVariableBitRate": true},
                    {"filename": "B\\V0\\2.mp3", "size": 1, "isVariableBitRate": true}]},
                {"username": "v2", "files": [
                    {"filename": "C\\V2\\1.mp3", "size": 1, "bitRate": 181, "isVariableBitRate": true},
                    {"filename": "C\\V2\\2.mp3", "size": 1, "bitRate": 203, "isVariableBitRate": true}]},
                {"username": "unknown", "files": [
                    {"filename": "D\\X\\1.mp3", "size": 1, "isVariableBitRate": true}]},
                {"username": "vorbis", "files": [
                    {"filename": "E\\Q5\\1.ogg", "size": 1, "bitRate": 150, "isVariableBitRate": true},
                    {"filename": "E\\Q5\\2.ogg", "size": 1, "bitRate": 170, "isVariableBitRate": true}]}
            ]"#,
            ),
        );
        let mut found = keys(&versions);
        found.sort_unstable();
        assert_eq!(
            found,
            ["mp3-vbr-190", "mp3-vbr-245", "mp3-vbr-x", "ogg-vbr-160"]
        );
        // One folder is one source, however much its files' averages differ.
        let v0 = versions
            .iter()
            .find(|version| version.key == "mp3-vbr-245")
            .unwrap();
        let users: Vec<&str> = v0
            .sources
            .iter()
            .map(|source| source.username.as_str())
            .collect();
        assert_eq!(users, ["v0", "v0b"]);
        assert!(v0.sources.iter().all(|source| source.files.len() == 2));
        assert_eq!(v0.quality.bitrate_kbps, Some(245));
        let unknown = versions
            .iter()
            .find(|version| version.key == "mp3-vbr-x")
            .unwrap();
        assert_eq!(unknown.quality.bitrate_kbps, None);
        assert_eq!(vbr_level("mp3", 300), 245);
        assert_eq!(vbr_level("mp3", 235), 245, "a tie goes to the higher level");
        assert_eq!(vbr_level("opus", 10), 32);
    }

    #[test]
    fn album_completeness_and_display_order() {
        let versions = group(
            Scope::Album,
            &responses(
                r#"[
                {"username": "slow", "hasFreeUploadSlot": true, "queueLength": 0, "uploadSpeed": 10, "files": [
                    {"filename": "S\\A\\1.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false},
                    {"filename": "S\\A\\2.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false}]},
                {"username": "fast", "hasFreeUploadSlot": true, "queueLength": 0, "uploadSpeed": 99, "files": [
                    {"filename": "F\\A\\1.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false},
                    {"filename": "F\\A\\2.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false}]},
                {"username": "queued", "hasFreeUploadSlot": true, "queueLength": 3, "uploadSpeed": 999, "files": [
                    {"filename": "Q\\A\\1.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false},
                    {"filename": "Q\\A\\2.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false}]},
                {"username": "busy", "hasFreeUploadSlot": false, "queueLength": 0, "uploadSpeed": 999, "files": [
                    {"filename": "B\\A\\1.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false},
                    {"filename": "B\\A\\2.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false}]},
                {"username": "partial", "hasFreeUploadSlot": true, "queueLength": 0, "uploadSpeed": 9999, "files": [
                    {"filename": "P\\A\\1.mp3", "size": 1, "bitRate": 320, "isVariableBitRate": false}]},
                {"username": "lossless1", "files": [{"filename": "L\\A\\1.flac", "size": 1, "bitDepth": 16, "sampleRate": 44100}]},
                {"username": "lossless2", "files": [{"filename": "L\\A\\1.flac", "size": 1, "bitDepth": 16, "sampleRate": 44100}]},
                {"username": "lossless3", "files": [{"filename": "L\\A\\1.flac", "size": 1, "bitDepth": 16, "sampleRate": 44100}]}
            ]"#,
            ),
        );
        // Four complete MP3 sources beat three complete FLAC sources.
        assert_eq!(keys(&versions), ["mp3-320-cbr", "flac-16-44100"]);
        let mp3 = &versions[0];
        assert_eq!((mp3.file_count, mp3.complete_source_count), (2, 4));
        let order: Vec<(&str, bool)> = mp3
            .sources
            .iter()
            .map(|source| (source.username.as_str(), source.complete))
            .collect();
        assert_eq!(
            order,
            [
                ("fast", true),
                ("slow", true),
                ("queued", true),
                ("busy", true),
                ("partial", false),
            ]
        );
    }

    #[test]
    fn track_scope_makes_one_complete_source_per_file() {
        let versions = group(
            Scope::Track,
            &responses(
                r#"[{"username": "alice", "files": [
                    {"filename": "A\\Single\\Song.flac", "size": 30, "bitDepth": 16, "sampleRate": 44100},
                    {"filename": "A\\Single\\Song (Live).flac", "size": 40, "bitDepth": 16, "sampleRate": 44100},
                    {"filename": "A\\Single\\cover.jpg", "size": 1}
                ]}]"#,
            ),
        );
        assert_eq!(versions.len(), 1);
        let version = &versions[0];
        assert_eq!((version.file_count, version.total_bytes), (1, 40));
        assert_eq!(version.complete_source_count, 2);
        for source in &version.sources {
            assert_eq!(source.files.len(), 1);
            assert_eq!(source.folder, r"A\Single");
            assert_eq!(source.artwork.len(), 1);
        }
        assert_ne!(version.sources[0].id, version.sources[1].id);
    }

    #[test]
    fn source_ids_and_version_keys_are_stable_and_responses_merge_by_user() {
        let fixture = r#"[
            {"username": "alice", "hasFreeUploadSlot": true, "files": [{"filename": "A/B/1.ogg", "size": 1, "bitRate": 160, "isVariableBitRate": false}]},
            {"username": "alice", "hasFreeUploadSlot": false, "files": [
                {"filename": "A/B/1.ogg", "size": 1, "bitRate": 160, "isVariableBitRate": false},
                {"filename": "A/B/2.ogg", "size": 1, "bitRate": 160, "isVariableBitRate": false}]}
        ]"#;
        let first = group(Scope::Album, &responses(fixture));
        assert_eq!(first, group(Scope::Album, &responses(fixture)));
        assert_eq!(keys(&first), ["ogg-160-cbr"]);
        let source = &first[0].sources[0];
        assert_eq!(first[0].sources.len(), 1);
        assert_eq!(source.folder, "A/B");
        assert_eq!(source.files.len(), 2);
        assert!(source.has_free_upload_slot);
        assert_eq!(source.id, source_id("alice", "A/B"));
        assert_eq!(source.id.len(), 16);
        assert_ne!(source_id("alice", "A/B"), source_id("alic", "eA/B"));
    }

    #[test]
    fn scopes_round_trip_their_names() {
        for scope in [Scope::Album, Scope::Track] {
            assert_eq!(Scope::parse(scope.name()), Some(scope));
        }
        assert_eq!(Scope::parse("single"), None);
    }
}
