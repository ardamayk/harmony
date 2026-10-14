package managedimport

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
	"github.com/ardam/navidrome-replacement/server/internal/searchdata"
	"github.com/google/uuid"
)

// ArtistCreditRepairReport is an in-memory preview, not an editable repair manifest.
// Apply revalidates its private snapshot. Run only while the Music Server is stopped.
// ponytail: offline operation only; online repair would require a shared file-operation lock.
type ArtistCreditRepairReport struct {
	Applied                   bool                      `json:"applied"`
	BackupPath                string                    `json:"backupPath,omitempty"`
	Changes                   []ArtistCreditRepairEntry `json:"changes"`
	Skipped                   []ArtistCreditRepairEntry `json:"skipped"`
	Conflicts                 []ArtistCreditRepairEntry `json:"conflicts"`
	databasePath, managedRoot string
	snapshot                  creditRepairSnapshot
	tracks                    map[string][]string
	albums                    map[string][]string
	albumKeys                 map[string]string
	ready                     bool
}

type ArtistCreditRepairEntry struct {
	Kind   string   `json:"kind"`
	ID     string   `json:"id"`
	Before []string `json:"before,omitempty"`
	After  []string `json:"after,omitempty"`
	Reason string   `json:"reason,omitempty"`
}

type repairCredit struct {
	ID, Name string
	Position int
}
type repairTrack struct {
	ID, AlbumID, ArtistName, Path, SourceID, SourceKind, SourcePath, Hash string
	Revision, SourceRevision, Size, SourceSize                            int64
	Hidden                                                                bool
	Credits                                                               []repairCredit
}
type repairAlbum struct {
	ID, Title, ArtistID, Key string
	Revision                 int64
	Credits                  []repairCredit
}
type creditRepairSnapshot struct {
	Tracks []repairTrack
	Albums []repairAlbum
}

func openCreditRepairDatabase(ctx context.Context, path string, writable bool) (*sql.DB, error) {
	if err := searchdata.CheckRegistration(); err != nil {
		return nil, err
	}
	info, err := os.Lstat(path)
	if err != nil {
		return nil, fmt.Errorf("open existing repair database: %w", err)
	}
	if !info.Mode().IsRegular() || info.Size() == 0 {
		return nil, errors.New("repair database must be an existing nonempty regular file")
	}
	mode := "ro"
	if writable {
		mode = "rw"
	}
	location := url.URL{Scheme: "file", Path: path}
	query := url.Values{"mode": {mode}, "_pragma": {"foreign_keys(1)", "busy_timeout(5000)"}}
	if writable {
		query.Set("_txlock", "immediate")
	}
	location.RawQuery = query.Encode()
	database, err := sql.Open("sqlite", location.String())
	if err != nil {
		return nil, err
	}
	database.SetMaxOpenConns(1)
	if err := database.PingContext(ctx); err != nil {
		return nil, errors.Join(err, database.Close())
	}
	return database, nil
}

func rejectPendingCreditRepair(ctx context.Context, q *db.Queries) error {
	pending, err := q.HasPendingFileOperations(ctx)
	if err != nil {
		return fmt.Errorf("check pending file operations: %w", err)
	}
	if deref(pending) {
		return errors.New("pending file operation journal; recover with the Music Server before repair")
	}
	return nil
}

func readCreditRepairSnapshot(ctx context.Context, q *db.Queries) (creditRepairSnapshot, error) {
	var snapshot creditRepairSnapshot
	trackRows, err := q.ListRepairTracks(ctx)
	if err != nil {
		return snapshot, err
	}
	for _, row := range trackRows {
		track := repairTrack{
			ID: row.ID, AlbumID: row.AlbumID, ArtistName: row.ArtistName, Path: row.FilePath,
			Revision: row.Revision, Size: row.SizeBytes, Hidden: row.Hidden != 0,
			SourceID: row.SourceID, SourceKind: row.SourceKind, SourcePath: row.SourcePath, Hash: row.SourceHash,
			SourceRevision: row.SourceRevision, SourceSize: row.SourceSize,
		}
		if err = json.Unmarshal([]byte(row.Credits), &track.Credits); err != nil {
			return snapshot, err
		}
		snapshot.Tracks = append(snapshot.Tracks, track)
	}
	albumRows, err := q.ListRepairAlbums(ctx)
	if err != nil {
		return snapshot, err
	}
	for _, row := range albumRows {
		album := repairAlbum{ID: row.ID, Title: row.Title, ArtistID: row.ArtistID, Key: row.IdentityKey, Revision: row.Revision}
		if err := json.Unmarshal([]byte(row.Credits), &album.Credits); err != nil {
			return snapshot, err
		}
		snapshot.Albums = append(snapshot.Albums, album)
	}
	return snapshot, nil
}

// PreviewArtistCreditRepair reads only authoritative managed files. It never migrates
// or writes the database; invalid files are reported without blocking safe Tracks.
func PreviewArtistCreditRepair(ctx context.Context, databasePath, managedRoot string) (report ArtistCreditRepairReport, returnErr error) {
	defer func() { report.ready = returnErr == nil }()
	report.Changes = []ArtistCreditRepairEntry{}
	report.Skipped = []ArtistCreditRepairEntry{}
	report.Conflicts = []ArtistCreditRepairEntry{}
	report.tracks = map[string][]string{}
	report.albums = map[string][]string{}
	report.albumKeys = map[string]string{}
	var err error
	report.databasePath, err = filepath.Abs(databasePath)
	if err != nil {
		return report, err
	}
	report.managedRoot, err = filepath.Abs(managedRoot)
	if err != nil || strings.TrimSpace(managedRoot) == "" {
		return report, errors.New("managed root is required")
	}
	rootInfo, err := os.Lstat(report.managedRoot)
	if err != nil {
		return report, fmt.Errorf("existing managed root required: %w", err)
	}
	if !rootInfo.IsDir() {
		return report, errors.New("managed root must be a directory, not a symlink")
	}
	database, err := openCreditRepairDatabase(ctx, report.databasePath, false)
	if err != nil {
		return report, err
	}
	defer func() { returnErr = errors.Join(returnErr, database.Close()) }()
	transaction, err := database.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return report, err
	}
	defer func() { returnErr = errors.Join(returnErr, rollbackTransaction(transaction, "artist credit preview")) }()
	readQueries := db.New(transaction)
	if err = rejectPendingCreditRepair(ctx, readQueries); err != nil {
		return report, err
	}
	report.snapshot, err = readCreditRepairSnapshot(ctx, readQueries)
	if err != nil {
		return report, err
	}
	if err = transaction.Commit(); err != nil {
		return report, err
	}
	storage := NewStorage(report.managedRoot, StorageLimits{FileBytes: 1, BatchBytes: 1})
	inspector := library.NewMediaInspector()
	inspected := map[string]library.NormalizedMediaMetadata{}
	for _, track := range report.snapshot.Tracks {
		if err = ctx.Err(); err != nil {
			return report, err
		}
		metadata, err := inspectCreditRepairTrack(ctx, storage, inspector, track)
		if err != nil {
			report.Skipped = append(report.Skipped, ArtistCreditRepairEntry{Kind: "track", ID: track.ID, Reason: err.Error()})
			continue
		}
		inspected[track.ID] = metadata
		if !sameRepairCredits(track.Credits, metadata.Artists) || track.ArtistName != strings.Join(metadata.Artists, ", ") {
			report.tracks[track.ID] = metadata.Artists
			report.Changes = append(report.Changes, ArtistCreditRepairEntry{Kind: "track", ID: track.ID, Before: repairCreditNames(track.Credits), After: slices.Clone(metadata.Artists)})
		}
	}
	occupied := map[string]string{}
	for _, album := range report.snapshot.Albums {
		if album.Key != "" {
			occupied[album.Key] = album.ID
		}
	}
	proposed := map[string][]string{}
	for _, album := range report.snapshot.Albums {
		names, reason := consensusRepairAlbum(album, report.snapshot.Tracks, inspected)
		if reason != "" {
			report.Skipped = append(report.Skipped, ArtistCreditRepairEntry{Kind: "album", ID: album.ID, Reason: reason})
			continue
		}
		if sameRepairCredits(album.Credits, names) && len(album.Credits) > 0 && album.ArtistID == album.Credits[0].ID {
			continue
		}
		key, err := repairAlbumIdentity(album, names)
		if err != nil {
			report.Conflicts = append(report.Conflicts, ArtistCreditRepairEntry{Kind: "album", ID: album.ID, Reason: err.Error()})
			continue
		}
		report.albums[album.ID] = names
		report.albumKeys[album.ID] = key
		proposed[key] = append(proposed[key], album.ID)
	}
	for _, album := range report.snapshot.Albums {
		names, ok := report.albums[album.ID]
		if !ok {
			continue
		}
		key := report.albumKeys[album.ID]
		if owner := occupied[key]; (owner != "" && owner != album.ID) || len(proposed[key]) > 1 {
			report.Conflicts = append(report.Conflicts, ArtistCreditRepairEntry{Kind: "album", ID: album.ID, Reason: "Album identity collision; no merge or split"})
			delete(report.albums, album.ID)
			delete(report.albumKeys, album.ID)
			continue
		}
		report.Changes = append(report.Changes, ArtistCreditRepairEntry{Kind: "album", ID: album.ID, Before: repairCreditNames(album.Credits), After: slices.Clone(names)})
	}
	return report, nil
}

func inspectCreditRepairTrack(ctx context.Context, storage *Storage, inspector library.MediaInspector, track repairTrack) (library.NormalizedMediaMetadata, error) {
	if track.Hidden {
		return library.NormalizedMediaMetadata{}, errors.New("track is not visible")
	}
	if track.SourceKind != "managed" || track.Path != track.SourcePath || track.Size != track.SourceSize {
		return library.NormalizedMediaMetadata{}, errors.New("authoritative managed source differs from Track")
	}
	_, size, err := storage.ResolveManagedFile(track.SourcePath, track.Hash)
	if err != nil {
		return library.NormalizedMediaMetadata{}, err
	}
	if size != track.SourceSize {
		return library.NormalizedMediaMetadata{}, errors.New("managed source size changed")
	}
	inspection, err := inspector.Inspect(ctx, track.SourcePath, nil)
	if err != nil {
		return library.NormalizedMediaMetadata{}, err
	}
	if inspection.FileSHA256 != track.Hash {
		return library.NormalizedMediaMetadata{}, errors.New("managed source changed during inspection")
	}
	if _, size, err = storage.ResolveManagedFile(track.SourcePath, track.Hash); err != nil {
		return library.NormalizedMediaMetadata{}, err
	}
	if size != track.SourceSize {
		return library.NormalizedMediaMetadata{}, errors.New("managed source size changed during inspection")
	}
	return inspection.Metadata, nil
}

// Apply backs up first, then atomically applies only the private preview plan.
// A stale database or changed source aborts the entire repair without metadata writes.
func (report *ArtistCreditRepairReport) Apply(ctx context.Context, backupPath string) (returnErr error) {
	if !report.ready || report.Applied {
		return errors.New("fresh artist credit preview required")
	}
	if len(report.tracks)+len(report.albums) == 0 {
		return nil
	}
	database, err := openCreditRepairDatabase(ctx, report.databasePath, true)
	if err != nil {
		return err
	}
	defer func() { returnErr = errors.Join(returnErr, database.Close()) }()
	if err = rejectPendingCreditRepair(ctx, db.New(database)); err != nil {
		return err
	}
	if strings.TrimSpace(backupPath) == "" {
		return errors.New("backup path is required")
	}
	backupPath, err = filepath.Abs(backupPath)
	if err != nil {
		return errors.New("backup path is required")
	}
	if err = backupCreditRepair(ctx, database, backupPath); err != nil {
		return fmt.Errorf("backup failed; no repair applied: %w", err)
	}
	report.BackupPath = backupPath
	storage := NewStorage(report.managedRoot, StorageLimits{FileBytes: 1, BatchBytes: 1})
	// Hashing remains outside the short database write transaction.
	for _, track := range report.snapshot.Tracks {
		if _, ok := report.tracks[track.ID]; !ok {
			if _, ok := report.albums[track.AlbumID]; !ok {
				continue
			}
		}
		if contextErr := ctx.Err(); contextErr != nil {
			return contextErr
		}
		_, size, resolveErr := storage.ResolveManagedFile(track.SourcePath, track.Hash)
		if resolveErr != nil {
			return fmt.Errorf("stale repair source %s: %w", track.ID, resolveErr)
		}
		if size != track.SourceSize {
			return fmt.Errorf("stale repair source %s: size changed", track.ID)
		}
	}
	transaction, err := database.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer func() { returnErr = errors.Join(returnErr, rollbackTransaction(transaction, "artist credit repair")) }()
	q := db.New(transaction)
	if err = rejectPendingCreditRepair(ctx, q); err != nil {
		return err
	}
	current, err := readCreditRepairSnapshot(ctx, q)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(report.snapshot, current) {
		return errors.New("stale artist credit preview; database changed")
	}
	oldArtists := []string{}
	for _, track := range report.snapshot.Tracks {
		names, ok := report.tracks[track.ID]
		if !ok {
			continue
		}
		ids, err := upsertRepairArtists(ctx, q, names)
		if err != nil {
			return err
		}
		if err := q.DeleteTrackArtists(ctx, track.ID); err != nil {
			return err
		}
		if err := insertTrackArtistCredits(ctx, q, track.ID, names, ids); err != nil {
			return err
		}
		if err := q.SetTrackArtistName(ctx, db.SetTrackArtistNameParams{ArtistName: strings.Join(names, ", "), ID: track.ID}); err != nil {
			return err
		}
		for _, credit := range track.Credits {
			oldArtists = append(oldArtists, credit.ID)
		}
	}
	for _, album := range report.snapshot.Albums {
		names, ok := report.albums[album.ID]
		if !ok {
			continue
		}
		ids, err := upsertRepairArtists(ctx, q, names)
		if err != nil {
			return err
		}
		if err := q.DeleteAlbumArtists(ctx, album.ID); err != nil {
			return err
		}
		if err := insertAlbumArtistCredits(ctx, q, album.ID, names, ids); err != nil {
			return err
		}
		albumKey := report.albumKeys[album.ID]
		if err := q.SetAlbumArtistIdentity(ctx, db.SetAlbumArtistIdentityParams{
			ArtistID: ids[normalizeIdentity(names[0])], IdentityKey: &albumKey, ID: album.ID,
		}); err != nil {
			return err
		}
		oldArtists = append(oldArtists, album.ArtistID)
		for _, credit := range album.Credits {
			oldArtists = append(oldArtists, credit.ID)
		}
	}
	// Existing cleanup checks relationships; protect the legacy primary-artist FK too.
	for _, id := range oldArtists {
		referenced, err := q.IsAlbumPrimaryArtist(ctx, id)
		if err != nil {
			return err
		}
		if !referenced {
			if err := q.DeleteArtistIfUnreferenced(ctx, id); err != nil {
				return fmt.Errorf("delete unreferenced Artist after Track Replacement: %w", err)
			}
		}
	}
	if err := transaction.Commit(); err != nil {
		return err
	}
	report.Applied = true
	return nil
}

func upsertRepairArtists(ctx context.Context, q *db.Queries, names []string) (map[string]string, error) {
	ids := map[string]string{}
	for _, name := range names {
		key := normalizeIdentity(name)
		id, err := upsertArtist(ctx, q, name, key, "")
		if err != nil {
			return nil, err
		}
		ids[key] = id
	}
	return ids, nil
}

func backupCreditRepair(ctx context.Context, database *sql.DB, path string) (returnErr error) {
	// Reserve a private empty destination: VACUUM INTO accepts an empty file,
	// never overwrites an existing backup, includes committed WAL contents.
	file, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_RDWR, 0o600)
	if err != nil {
		return err
	}
	defer func() { returnErr = errors.Join(returnErr, file.Close()) }()
	if _, err = database.ExecContext(ctx, `VACUUM INTO ?`, path); err != nil {
		return err
	}
	if err = file.Sync(); err != nil {
		return err
	}
	backup, err := openCreditRepairDatabase(ctx, path, false)
	if err != nil {
		return err
	}
	defer func() { returnErr = errors.Join(returnErr, backup.Close()) }()
	var integrity string
	if err = backup.QueryRowContext(ctx, `PRAGMA integrity_check`).Scan(&integrity); err != nil {
		return err
	}
	if integrity != "ok" {
		return fmt.Errorf("backup integrity: %s", integrity)
	}
	rows, err := backup.QueryContext(ctx, `PRAGMA foreign_key_check`)
	if err != nil {
		return err
	}
	defer func() { returnErr = errors.Join(returnErr, rows.Close()) }()
	if rows.Next() {
		return errors.New("backup foreign key check failed")
	}
	return rows.Err()
}

func repairCreditNames(credits []repairCredit) []string {
	names := make([]string, len(credits))
	for index, credit := range credits {
		names[index] = credit.Name
	}
	return names
}
func sameRepairCredits(credits []repairCredit, names []string) bool {
	if len(credits) != len(names) {
		return false
	}
	for index, credit := range credits {
		if credit.Position != index || normalizeIdentity(credit.Name) != normalizeIdentity(names[index]) {
			return false
		}
	}
	return true
}
func consensusRepairAlbum(album repairAlbum, tracks []repairTrack, inspected map[string]library.NormalizedMediaMetadata) ([]string, string) {
	var names []string
	for _, track := range tracks {
		if track.AlbumID != album.ID {
			continue
		}
		metadata, ok := inspected[track.ID]
		if !ok {
			return nil, "not all Album members were safely inspected"
		}
		if names == nil {
			names = metadata.AlbumArtists
		} else if !slices.Equal(names, metadata.AlbumArtists) {
			return nil, "Album members disagree on Album Artist credits"
		}
	}
	if len(names) == 0 {
		return nil, "Album has no inspected members"
	}
	return names, ""
}
func repairAlbumIdentity(album repairAlbum, names []string) (string, error) {
	oldKey := albumIdentityKey(library.NormalizedMediaMetadata{Album: album.Title, AlbumArtists: repairCreditNames(album.Credits)})
	suffix := strings.TrimPrefix(album.Key, oldKey)
	if album.Key != oldKey {
		edition, ok := strings.CutPrefix(suffix, "\x1fmanaged-import-edition:")
		if !strings.HasPrefix(album.Key, oldKey) || !ok || uuid.Validate(edition) != nil {
			return "", errors.New("unrecognized old Album identity; manual review required")
		}
	}
	return albumIdentityKey(library.NormalizedMediaMetadata{Album: album.Title, AlbumArtists: names}) + suffix, nil
}
