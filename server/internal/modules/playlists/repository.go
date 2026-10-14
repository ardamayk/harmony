package playlists

import (
	"context"
	"database/sql"
	"errors"

	"github.com/ardam/navidrome-replacement/server/internal/modules/playlists/db"
	"github.com/google/uuid"
)

// Repository maps the generated playlist queries to domain types.
type Repository struct {
	q *db.Queries
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{q: db.New(sqlDB)}
}

func (r *Repository) EnsureDefault(ctx context.Context, userID string) (Playlist, error) {
	if err := r.q.EnsureDefaultPlaylist(ctx, db.EnsureDefaultPlaylistParams{
		ID: uuid.NewString(), UserID: userID, Name: DefaultFavoritesName,
	}); err != nil {
		return Playlist{}, err
	}
	row, err := r.q.GetPlaylistByName(ctx, db.GetPlaylistByNameParams{UserID: userID, Name: DefaultFavoritesName})
	if err != nil {
		return Playlist{}, err
	}
	return toPlaylist(row.ID, row.Name, row.IsDefault, row.TrackCount), nil
}

func (r *Repository) List(ctx context.Context, userID string) ([]Playlist, error) {
	rows, err := r.q.ListPlaylists(ctx, userID)
	if err != nil {
		return nil, err
	}
	items := make([]Playlist, 0, len(rows))
	for _, row := range rows {
		items = append(items, toPlaylist(row.ID, row.Name, row.IsDefault, row.TrackCount))
	}
	return items, nil
}

func (r *Repository) Create(ctx context.Context, userID, name string) (string, error) {
	id := uuid.NewString()
	return id, r.q.InsertPlaylist(ctx, db.InsertPlaylistParams{ID: id, UserID: userID, Name: name})
}

func (r *Repository) Get(ctx context.Context, userID, playlistID string) (Playlist, error) {
	row, err := r.q.GetPlaylist(ctx, db.GetPlaylistParams{UserID: userID, ID: playlistID})
	if errors.Is(err, sql.ErrNoRows) {
		return Playlist{}, ErrNotFound
	}
	if err != nil {
		return Playlist{}, err
	}
	return toPlaylist(row.ID, row.Name, row.IsDefault, row.TrackCount), nil
}

func (r *Repository) TrackIDs(ctx context.Context, playlistID string) ([]string, error) {
	return r.q.ListPlaylistTrackIDs(ctx, playlistID)
}

// AppendTrack adds the track at the end; adding a present track is a no-op.
func (r *Repository) AppendTrack(ctx context.Context, playlistID, trackID string) error {
	return r.q.AppendPlaylistTrack(ctx, db.AppendPlaylistTrackParams{PlaylistID: playlistID, TrackID: trackID})
}

func (r *Repository) RemoveTrack(ctx context.Context, playlistID, trackID string) error {
	return r.q.DeletePlaylistTrack(ctx, db.DeletePlaylistTrackParams{PlaylistID: playlistID, TrackID: trackID})
}

func (r *Repository) CountTracks(ctx context.Context, playlistID string) (int64, error) {
	return r.q.CountPlaylistTracks(ctx, playlistID)
}

// DeleteUserPlaylist never deletes the default Favorites playlist.
func (r *Repository) DeleteUserPlaylist(ctx context.Context, userID, playlistID string) error {
	return r.q.DeleteUserPlaylist(ctx, db.DeleteUserPlaylistParams{ID: playlistID, UserID: userID})
}

func toPlaylist(id, name string, isDefault, trackCount int64) Playlist {
	return Playlist{ID: id, Name: name, IsDefault: isDefault != 0, TrackCount: int(trackCount)}
}
