//! Managed Storage behaviour (storage_safety_test.go, capacity_writer_test.go
//! and the storage half of the replacement tests).

use super::errors::Sentinel;
use super::storage::*;
use super::types::*;
use crate::inspect::{
    AlbumArtwork, MediaInspection, MediaPosition, NormalizedMetadata, TechnicalAudio,
};
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Arc, Mutex};

const LIMITS: Limits = Limits {
    reserve_bytes: 0,
    file_bytes: 1 << 30,
    batch_bytes: 1 << 31,
};

fn sha(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn plenty() -> Capacity {
    Arc::new(|_: &Path| Ok(i64::MAX / 2))
}

fn storage_in(dir: &Path) -> Storage {
    Storage::with_capacity(dir.join("managed").to_str().unwrap(), LIMITS, plenty())
}

fn body(
    chunks: Vec<&[u8]>,
) -> impl futures_util::Stream<Item = super::errors::Result<Bytes>> + Unpin {
    futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>(),
    )
}

async fn stage(storage: &Storage, data: &[u8]) -> StagedUpload {
    storage
        .stage_upload(body(vec![data]), data.len() as i64)
        .await
        .unwrap()
}

fn inspection(artwork: &[u8]) -> MediaInspection {
    MediaInspection {
        metadata: NormalizedMetadata {
            title: "Song Ünïcode!".into(),
            artists: vec!["A".into()],
            album_artists: vec!["The Band".into()],
            album: "Best Of".into(),
            track_position: MediaPosition {
                number: 3,
                total: 0,
            },
            disc_position: MediaPosition {
                number: 1,
                total: 0,
            },
            ..NormalizedMetadata::default()
        },
        album_artwork: if artwork.is_empty() {
            AlbumArtwork::default()
        } else {
            AlbumArtwork {
                mime_type: "image/png".into(),
                data: artwork.to_vec(),
                sha256: sha(artwork),
                ..AlbumArtwork::default()
            }
        },
        audio: TechnicalAudio {
            format: "flac".into(),
            ..TechnicalAudio::default()
        },
        file_sha256: String::new(),
    }
}

fn identity() -> CommitIdentity {
    CommitIdentity {
        album_artist_id: "artist1".into(),
        album_id: "album1".into(),
        track_id: "track1".into(),
        ..CommitIdentity::default()
    }
}

#[tokio::test]
async fn stages_uploads_with_their_hash_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage_in(dir.path());
    let staged = storage
        .stage_upload(body(vec![b"abc", b"def"]), -1)
        .await
        .unwrap();
    assert_eq!(staged.size, 6);
    assert_eq!(staged.sha256, sha(b"abcdef"));
    assert!(staged.path.contains("/managed/.staging/.import-"));
    assert_eq!(std::fs::read(&staged.path).unwrap(), b"abcdef");
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(Path::new(&staged.path)), 0o600);
    assert_eq!(mode(&dir.path().join("managed/.staging")), 0o700);
    assert_eq!(storage.staged_file_size(&staged.path).unwrap(), 6);
    storage.remove_all_staged().unwrap();
    assert!(!Path::new(&staged.path).exists());
}

#[tokio::test]
async fn rejects_oversized_short_and_failed_uploads_leaving_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits {
        reserve_bytes: 0,
        file_bytes: 4,
        batch_bytes: 10,
    };
    let storage = Storage::with_capacity(dir.path().join("m").to_str().unwrap(), limits, plenty());
    let declared = storage.stage_upload(body(vec![b"x"]), 5).await.unwrap_err();
    assert!(declared.is(Sentinel::UploadTooLarge));
    let streamed = storage
        .stage_upload(body(vec![b"abc", b"de"]), -1)
        .await
        .unwrap_err();
    assert!(streamed.is(Sentinel::UploadTooLarge));
    let short = storage
        .stage_upload(body(vec![b"ab"]), 3)
        .await
        .unwrap_err();
    assert!(short.is(Sentinel::UploadInterrupted));
    let failing = futures_util::stream::iter(vec![
        Ok(Bytes::from_static(b"a")),
        Err(super::errors::Error::wrap(
            Sentinel::UploadInterrupted,
            "connection reset",
        )),
    ]);
    let failed = storage.stage_upload(failing, -1).await.unwrap_err();
    assert!(failed.is(Sentinel::UploadInterrupted));
    let staged: Vec<_> = std::fs::read_dir(dir.path().join("m/.staging"))
        .unwrap()
        .collect();
    assert!(staged.is_empty());
}

#[tokio::test]
async fn probes_capacity_per_interval_and_stops_when_it_runs_out() {
    let dir = tempfile::tempdir().unwrap();
    let probes = Arc::new(Mutex::new(Vec::new()));
    let available = Arc::new(Mutex::new(i64::MAX / 2));
    let capacity: Capacity = {
        let available = available.clone();
        Arc::new(move |_: &Path| Ok(*available.lock().unwrap()))
    };
    let recorder = probes.clone();
    let recording: Capacity = Arc::new(move |path: &Path| {
        let value = capacity(path)?;
        recorder.lock().unwrap().push(value);
        Ok(value)
    });
    let storage = Storage::with_capacity(dir.path().join("m").to_str().unwrap(), LIMITS, recording);
    let chunk = vec![0u8; 32 * 1024];
    let chunks: Vec<&[u8]> = (0..256).map(|_| chunk.as_slice()).collect();
    storage
        .stage_upload(body(chunks.clone()), 8 << 20)
        .await
        .unwrap();
    // One probe before the upload, then one per 4 MiB instead of per chunk.
    assert!(
        probes.lock().unwrap().len() <= 4,
        "{}",
        probes.lock().unwrap().len()
    );
    *available.lock().unwrap() = 1024;
    let error = storage.stage_upload(body(chunks), -1).await.unwrap_err();
    assert!(error.is(Sentinel::InsufficientStorage));
}

#[test]
fn preflight_keeps_the_reserve() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits {
        reserve_bytes: 100,
        ..LIMITS
    };
    let storage = Storage::with_capacity(
        dir.path().join("m").to_str().unwrap(),
        limits,
        Arc::new(|_: &Path| Ok(150)),
    );
    let requirement = |selected, temporary| Requirement {
        selected_bytes: selected,
        temporary_bytes: temporary,
    };
    assert!(storage.preflight(requirement(50, 0)).is_ok());
    assert!(
        storage
            .preflight(requirement(40, 11))
            .unwrap_err()
            .is(Sentinel::InsufficientStorage)
    );
}

#[test]
fn refuses_a_root_that_is_or_sits_under_a_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, dir.path().join("link")).unwrap();
    for root in [dir.path().join("link"), dir.path().join("link/managed")] {
        let storage = Storage::with_capacity(root.to_str().unwrap(), LIMITS, plenty());
        let error = storage.preflight(Requirement::default()).unwrap_err();
        assert!(error.is(Sentinel::UnsafeStoragePath), "{root:?}: {error}");
    }
    assert!(!outside.join("managed").exists());
    let filesystem_root = Storage::with_capacity("/", LIMITS, plenty());
    assert!(
        filesystem_root
            .preflight(Requirement::default())
            .unwrap_err()
            .is(Sentinel::UnsafeStoragePath)
    );
}

#[tokio::test]
async fn refuses_staging_and_library_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let managed = dir.path().join("managed");
    std::fs::create_dir(&managed).unwrap();
    std::os::unix::fs::symlink(&outside, managed.join(".staging")).unwrap();
    let storage = storage_in(dir.path());
    let error = storage.stage_upload(body(vec![b"a"]), 1).await.unwrap_err();
    assert!(error.is(Sentinel::UnsafeStoragePath), "{error}");
    std::fs::remove_file(managed.join(".staging")).unwrap();
    let staged = stage(&storage, b"audio").await;
    std::os::unix::fs::symlink(&outside, managed.join("library")).unwrap();
    let error = storage
        .place(&staged.path, &inspection(b""), &identity(), None)
        .unwrap_err();
    assert!(error.is(Sentinel::UnsafeStoragePath), "{error}");
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    assert!(
        storage
            .relative_path("/etc/passwd")
            .unwrap_err()
            .is(Sentinel::UnsafeStoragePath)
    );
    assert!(
        storage
            .relative_path(managed.join("../x").to_str().unwrap())
            .is_err()
    );
}

#[tokio::test]
async fn places_verifies_and_rolls_back_a_track() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage_in(dir.path());
    let staged = stage(&storage, b"audio").await;
    let created = Mutex::new(false);
    let mark = || -> super::errors::Result<()> {
        *created.lock().unwrap() = true;
        Ok(())
    };
    let placed = storage
        .place(
            &staged.path,
            &inspection(b"cover"),
            &identity(),
            Some(&mark),
        )
        .unwrap();
    assert!(*created.lock().unwrap());
    assert!(placed.artwork_created);
    assert!(
        placed.audio_path.ends_with(
            "managed/library/the-band-artist1/best-of-album1/01-03-song-n-code-track1.flac"
        ),
        "{}",
        placed.audio_path
    );
    assert!(placed.artwork_path.ends_with("best-of-album1/cover.png"));
    storage
        .verify_placement(&placed, &sha(b"audio"), &sha(b"cover"))
        .unwrap();
    assert!(
        storage
            .verify_placement(&placed, &sha(b"other"), &sha(b"cover"))
            .is_err()
    );
    storage.rollback(&placed).unwrap();
    assert_eq!(std::fs::read(&staged.path).unwrap(), b"audio");
    assert!(!dir.path().join("managed/library/the-band-artist1").exists());
    assert!(dir.path().join("managed/library").exists());
}

#[tokio::test]
async fn keeps_matching_artwork_and_refuses_a_different_one() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage_in(dir.path());
    let first = storage
        .place(
            &stage(&storage, b"one").await.path,
            &inspection(b"cover"),
            &identity(),
            None,
        )
        .unwrap();
    let mut second_identity = identity();
    second_identity.track_id = "track2".into();
    let mut second_inspection = inspection(b"cover");
    second_inspection.metadata.track_position.number = 4;
    let second = storage
        .place(
            &stage(&storage, b"two").await.path,
            &second_inspection,
            &second_identity,
            None,
        )
        .unwrap();
    assert!(!second.artwork_created);
    assert_eq!(second.artwork_path, first.artwork_path);
    let mut third_identity = identity();
    third_identity.track_id = "track3".into();
    let mut third = inspection(b"other cover");
    third.metadata.track_position.number = 5;
    let error = storage
        .place(
            &stage(&storage, b"three").await.path,
            &third,
            &third_identity,
            None,
        )
        .unwrap_err();
    assert_eq!(
        error.as_validation().unwrap().code,
        ERROR_CODE_ALBUM_ARTWORK_CONFLICT
    );
}

#[test]
fn does_not_delete_a_managed_file_once_cancelled_or_changed() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage_in(dir.path());
    let file = dir.path().join("managed/library/a/b/x.flac");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"data").unwrap();
    let path = file.to_str().unwrap();
    let error = storage
        .remove_managed_file(&|| true, path, &sha(b"data"))
        .unwrap_err();
    assert!(error.is(Sentinel::Cancelled));
    let changed = storage
        .remove_managed_file(&|| false, path, &sha(b"other"))
        .unwrap_err();
    assert!(changed.is(Sentinel::UnsafeStoragePath));
    assert!(file.exists());
    assert!(
        storage
            .remove_managed_file(&|| false, path, &sha(b"data"))
            .unwrap()
    );
    assert!(!file.exists());
    assert!(
        storage
            .remove_managed_file(&|| false, path, &sha(b"data"))
            .unwrap()
    );
}

#[tokio::test]
async fn replaces_a_track_and_undoes_a_half_finished_swap() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage_in(dir.path());
    let original = storage
        .place(
            &stage(&storage, b"old audio").await.path,
            &inspection(b"cover"),
            &identity(),
            None,
        )
        .unwrap();
    let target = ReplacementTarget {
        track_id: "track1".into(),
        album_id: "album1".into(),
        relative_path: storage.relative_path(&original.audio_path).unwrap(),
        artwork_path: original.artwork_path.clone(),
        is_sole_track: true,
    };
    let mut replacement_identity = identity();
    replacement_identity.existing_artwork_path = original.artwork_path.clone();
    replacement_identity.existing_artwork_sha256 = sha(b"cover");
    let mut replacement = inspection(b"new cover");
    replacement.audio.format = "mp3".into();
    let staged = stage(&storage, b"new audio").await;
    let plan = storage
        .plan_replacement_placement(&staged.path, &replacement, &replacement_identity, &target)
        .unwrap();
    assert_eq!(plan.artwork_mode, REPLACEMENT_ARTWORK_MODE_REPLACE);
    let placed = storage
        .place_replacement(plan, &replacement, &replacement_identity, None)
        .unwrap();
    storage
        .verify_replacement(&placed, &sha(b"new audio"), &sha(b"new cover"))
        .unwrap();
    storage.swap_replacement(&placed).unwrap();
    assert_eq!(std::fs::read(&placed.audio_path).unwrap(), b"new audio");
    assert_eq!(std::fs::read(&original.artwork_path).unwrap(), b"new cover");
    // A crash after the swap: rollback restores the previous files.
    storage.rollback_replacement(&placed).unwrap();
    assert_eq!(std::fs::read(&original.audio_path).unwrap(), b"old audio");
    assert_eq!(std::fs::read(&original.artwork_path).unwrap(), b"cover");
    assert_eq!(std::fs::read(&staged.path).unwrap(), b"new audio");
    // Run it through to the end this time.
    let plan = storage
        .plan_replacement_placement(&staged.path, &replacement, &replacement_identity, &target)
        .unwrap();
    let placed = storage
        .place_replacement(plan, &replacement, &replacement_identity, None)
        .unwrap();
    storage.swap_replacement(&placed).unwrap();
    storage
        .stream_swapped_replacement(&placed, &sha(b"new audio"))
        .unwrap();
    let paths = storage.replacement_journal_paths(&placed);
    let journal = ReplacementJournal {
        retired_audio_path: paths.retired_audio,
        previous_audio_sha256: sha(b"old audio"),
        retired_artwork_path: paths.retired_artwork,
        previous_artwork_sha256: sha(b"cover"),
        ..ReplacementJournal::default()
    };
    let (deleted, result) = storage.complete_replacement_files(&|| false, &journal, &placed);
    result.unwrap();
    assert_eq!(deleted, 2);
    let album = Path::new(&placed.audio_path).parent().unwrap().to_owned();
    let mut names: Vec<_> = std::fs::read_dir(album)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["01-03-song-n-code-track1.mp3", "cover.png"]);
}

#[test]
fn slugs_like_go() {
    assert_eq!(slug("Hello, World!"), "hello-world");
    assert_eq!(slug("Beyoncé"), "beyonc");
    assert_eq!(slug("İstanbul"), "istanbul");
    assert_eq!(slug("!!!"), "untitled");
    assert_eq!(slug(&"ab-".repeat(40)).len(), 80);
    assert_eq!(
        slug(&"abc-".repeat(30)),
        format!("{}abc", "abc-".repeat(19))
    );
}
