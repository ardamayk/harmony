//! Prints the Media Inspection of each file as one JSON line, in the shape
//! the Go parity check prints, for comparing the two servers.

use earthly_music_server::inspect::{self, Context};
use serde_json::json;
use std::path::Path;

fn main() {
    for path in std::env::args().skip(1) {
        let never = || false;
        let mut ctx = Context::new(&never, None);
        let result = match inspect::inspect(Path::new(&path), &mut ctx) {
            Err(failure) => json!({ "path": path, "issues": failure.issues() }),
            Ok(inspection) => {
                let m = &inspection.metadata;
                let a = &inspection.album_artwork;
                let au = &inspection.audio;
                json!({
                    "path": path,
                    "metadata": {
                        "title": m.title, "artists": m.artists, "albumArtists": m.album_artists,
                        "album": m.album, "track": [m.track_position.number, m.track_position.total],
                        "disc": [m.disc_position.number, m.disc_position.total], "hasDisc": m.has_disc_number,
                        "genres": m.genres, "year": m.year, "replayGain": m.replay_gain, "lyrics": m.lyrics,
                    },
                    "artwork": { "warning": a.warning, "mime": a.mime_type, "width": a.width, "height": a.height, "sha256": a.sha256 },
                    "audio": {
                        "format": au.format, "container": au.container, "codec": au.codec,
                        "durationMs": au.duration_ms, "sampleRateHz": au.sample_rate_hz,
                        "channels": au.channel_count, "bitDepth": au.bit_depth, "bitrateKbps": au.bitrate_kbps,
                    },
                    "sha256": inspection.file_sha256,
                })
            }
        };
        println!("{result}");
    }
}
