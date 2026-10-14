package testutil

import (
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"

	"github.com/google/uuid"
	"golang.org/x/text/cases"
	"golang.org/x/text/unicode/norm"
)

// ManagedTrackSpec describes one Managed Track to seed directly into the
// expanded library model, the way a committed Managed Import leaves it.
// Zero-valued fields get defaults (see withManagedTrackDefaults); the managed
// source row receives a stable per-track content hash so several specs can
// coexist under the unique content constraint.
type ManagedTrackSpec struct {
	TrackID      string
	AlbumID      string
	Title        string
	Artist       string
	AlbumArtist  string
	Album        string
	TrackNo      int
	DiscNo       int
	Year         int
	DurationMs   int
	Format       string
	SizeBytes    int64
	FilePath     string
	Genres       []string
	SampleRateHz int
	BitDepth     int
	ReplayGain   ReplayGainSpec
}

// ReplayGainSpec carries optional ReplayGain columns for SeedManagedTrack.
type ReplayGainSpec struct {
	TrackGainDB *float64
	TrackPeak   *float64
	AlbumGainDB *float64
	AlbumPeak   *float64
}

// SeedManagedTrack inserts the artist, album, track, managed track source,
// credits and genres for one Managed Track and returns its album and track
// IDs. Artists, albums and genres are reused by normalized name so several
// specs can share an album or a credit.
func SeedManagedTrack(t *testing.T, db *sql.DB, spec ManagedTrackSpec) (albumID, trackID string) {
	t.Helper()
	spec = withManagedTrackDefaults(spec)

	albumArtistID := upsertSeedArtist(t, db, spec.AlbumArtist)
	trackArtistID := upsertSeedArtist(t, db, spec.Artist)
	albumID = upsertSeedAlbum(t, db, spec, albumArtistID)
	trackID = spec.TrackID

	seedExec(t, db, `
		INSERT INTO tracks (
			id, album_id, title, title_sort, artist_name, track_no, disc_no, duration_ms, format,
			size_bytes, file_path, file_mtime, genre, sample_rate_hz, bit_depth,
			replaygain_track_gain_db, replaygain_track_peak, replaygain_album_gain_db, replaygain_album_peak
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?)`,
		trackID, albumID, spec.Title, sortSeedName(spec.Title), spec.Artist, nullablePositive(spec.TrackNo),
		nullablePositive(spec.DiscNo), spec.DurationMs, spec.Format, spec.SizeBytes, spec.FilePath,
		nullableString(strings.Join(spec.Genres, ", ")), nullablePositive(spec.SampleRateHz), nullablePositive(spec.BitDepth),
		spec.ReplayGain.TrackGainDB, spec.ReplayGain.TrackPeak, spec.ReplayGain.AlbumGainDB, spec.ReplayGain.AlbumPeak,
	)
	seedExec(t, db, `
		INSERT INTO track_sources (id, track_id, source_kind, file_path, content_sha256, source_format, size_bytes)
		VALUES (?, ?, 'managed', ?, ?, ?, ?)`,
		uuid.NewString(), trackID, spec.FilePath, seedContentHash(trackID), spec.Format, spec.SizeBytes,
	)
	seedExec(t, db, `INSERT INTO track_artists (track_id, artist_id, position) VALUES (?, ?, 0)`, trackID, trackArtistID)
	seedExec(t, db, `INSERT OR IGNORE INTO album_artists (album_id, artist_id, position) VALUES (?, ?, 0)`, albumID, albumArtistID)
	for position, genre := range spec.Genres {
		genreID := upsertSeedGenre(t, db, genre)
		seedExec(t, db, `INSERT INTO track_genres (track_id, genre_id, position) VALUES (?, ?, ?)`, trackID, genreID, position)
	}
	refreshSeedAlbumGenres(t, db, albumID)
	return albumID, trackID
}

func withManagedTrackDefaults(spec ManagedTrackSpec) ManagedTrackSpec {
	if spec.TrackID == "" {
		spec.TrackID = uuid.NewString()
	}
	if spec.Title == "" {
		spec.Title = "Song"
	}
	if spec.Artist == "" {
		spec.Artist = "Artist"
	}
	if spec.AlbumArtist == "" {
		spec.AlbumArtist = spec.Artist
	}
	if spec.Album == "" {
		spec.Album = "Album"
	}
	// The strict position triggers reject a disc without a track number, so a
	// disc is only assumed when the spec places the track.
	if spec.DiscNo <= 0 && spec.TrackNo > 0 {
		spec.DiscNo = 1
	}
	if spec.Format == "" {
		spec.Format = "flac"
	}
	if spec.FilePath == "" {
		spec.FilePath = "/managed/" + spec.TrackID + "." + spec.Format
	}
	if spec.DurationMs <= 0 {
		spec.DurationMs = 1000
	}
	if spec.SizeBytes <= 0 {
		spec.SizeBytes = 10
	}
	return spec
}

func upsertSeedArtist(t *testing.T, db *sql.DB, name string) string {
	t.Helper()
	normalized := normalizeSeedIdentity(name)
	var id string
	err := db.QueryRow(`SELECT id FROM artists WHERE name_normalized = ?`, normalized).Scan(&id)
	if err == nil {
		return id
	}
	if err != sql.ErrNoRows {
		t.Fatalf("lookup seed artist %q: %v", name, err)
	}
	id = uuid.NewString()
	seedExec(t, db, `INSERT INTO artists (id, name, name_sort, name_normalized) VALUES (?, ?, ?, ?)`, id, name, sortSeedName(name), normalized)
	return id
}

func upsertSeedAlbum(t *testing.T, db *sql.DB, spec ManagedTrackSpec, artistID string) string {
	t.Helper()
	if spec.AlbumID != "" {
		return spec.AlbumID
	}
	var id string
	err := db.QueryRow(`SELECT id FROM albums WHERE artist_id = ? AND title_sort = ?`, artistID, sortSeedName(spec.Album)).Scan(&id)
	if err == nil {
		return id
	}
	if err != sql.ErrNoRows {
		t.Fatalf("lookup seed album %q: %v", spec.Album, err)
	}
	id = uuid.NewString()
	seedExec(t, db, `INSERT INTO albums (id, artist_id, title, title_sort, year, genres) VALUES (?, ?, ?, ?, ?, '[]')`,
		id, artistID, spec.Album, sortSeedName(spec.Album), nullablePositive(spec.Year))
	return id
}

func upsertSeedGenre(t *testing.T, db *sql.DB, name string) string {
	t.Helper()
	normalized := normalizeSeedIdentity(name)
	var id string
	err := db.QueryRow(`SELECT id FROM genres WHERE name_normalized = ?`, normalized).Scan(&id)
	if err == nil {
		return id
	}
	if err != sql.ErrNoRows {
		t.Fatalf("lookup seed genre %q: %v", name, err)
	}
	id = uuid.NewString()
	seedExec(t, db, `INSERT INTO genres (id, name, name_normalized) VALUES (?, ?, ?)`, id, name, normalized)
	return id
}

// refreshSeedAlbumGenres mirrors the album-level genre summary the library
// keeps in albums.genres: the ordered distinct genre names of its tracks.
func refreshSeedAlbumGenres(t *testing.T, db *sql.DB, albumID string) {
	t.Helper()
	rows, err := db.Query(`
		SELECT genres.name FROM track_genres
		JOIN genres ON genres.id = track_genres.genre_id
		JOIN tracks ON tracks.id = track_genres.track_id
		WHERE tracks.album_id = ?
		GROUP BY genres.id ORDER BY MIN(track_genres.position), genres.name`, albumID)
	if err != nil {
		t.Fatalf("read seed album genres: %v", err)
	}
	defer func() { _ = rows.Close() }()
	names := []string{}
	for rows.Next() {
		var name string
		if scanErr := rows.Scan(&name); scanErr != nil {
			t.Fatalf("scan seed album genre: %v", scanErr)
		}
		names = append(names, name)
	}
	encoded, err := json.Marshal(names)
	if err != nil {
		t.Fatalf("encode seed album genres: %v", err)
	}
	seedExec(t, db, `UPDATE albums SET genres = ? WHERE id = ?`, string(encoded), albumID)
}

func seedExec(t *testing.T, db *sql.DB, query string, args ...any) {
	t.Helper()
	if _, err := db.Exec(query, args...); err != nil {
		t.Fatalf("seed managed track: %v", err)
	}
}

// normalizeSeedIdentity mirrors managedimport.normalizeIdentity, which cannot
// be imported here without a cycle; keep the two in step.
func normalizeSeedIdentity(value string) string {
	return cases.Fold().String(strings.Join(strings.Fields(norm.NFC.String(value)), " "))
}

// sortSeedName lowercases and trims a seeded name for its *_sort column.
func sortSeedName(value string) string {
	return strings.ToLower(strings.TrimSpace(value))
}

func nullablePositive(value int) any {
	if value <= 0 {
		return nil
	}
	return value
}

func nullableString(value string) any {
	if strings.TrimSpace(value) == "" {
		return nil
	}
	return value
}

// seedContentHash derives a stable, per-track content hash so several seeded
// tracks satisfy the unique managed content constraint.
func seedContentHash(trackID string) string {
	sum := sha256.Sum256([]byte(trackID))
	return hex.EncodeToString(sum[:])
}
