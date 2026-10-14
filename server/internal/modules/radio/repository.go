package radio

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/ardam/navidrome-replacement/server/internal/modules/radio/db"
	"github.com/google/uuid"
)

// Repository maps saved radio stations between the database and domain types.
type Repository struct {
	q *db.Queries
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{q: db.New(sqlDB)}
}

func (r *Repository) List(ctx context.Context, userID string) ([]Station, error) {
	rows, err := r.q.ListStations(ctx, userID)
	if err != nil {
		return nil, err
	}
	items := make([]Station, 0, len(rows))
	for _, row := range rows {
		items = append(items, toStation(row))
	}
	return items, nil
}

func (r *Repository) Get(ctx context.Context, userID, stationID string) (Station, error) {
	return stationOrNotFound(r.q.GetStation(ctx, db.GetStationParams{UserID: userID, ID: stationID}))
}

func (r *Repository) GetByExternalID(ctx context.Context, userID, source, externalID string) (Station, error) {
	if externalID == "" {
		return Station{}, ErrNotFound
	}
	return stationOrNotFound(r.q.GetStationByExternalID(ctx, db.GetStationByExternalIDParams{
		UserID: userID, Source: source, ExternalID: externalID,
	}))
}

// Insert appends the station after the user's existing stations.
func (r *Repository) Insert(ctx context.Context, userID string, input StationCreate) (string, error) {
	position, err := r.q.NextStationPosition(ctx, userID)
	if err != nil {
		return "", err
	}
	tags, err := encodeTags(input.Tags)
	if err != nil {
		return "", err
	}
	id := uuid.NewString()
	return id, r.q.InsertStation(ctx, db.InsertStationParams{
		ID:          id,
		UserID:      userID,
		Name:        input.Name,
		StreamUrl:   input.StreamURL,
		HomepageUrl: input.HomepageURL,
		FaviconUrl:  input.FaviconURL,
		Country:     input.Country,
		Language:    input.Language,
		Tags:        tags,
		Codec:       input.Codec,
		Bitrate:     int64(input.Bitrate),
		Source:      input.Source,
		ExternalID:  input.ExternalID,
		IsFavorite:  boolInt(input.IsFavorite),
		Position:    position,
	})
}

func (r *Repository) Update(ctx context.Context, userID string, station Station) error {
	tags, err := encodeTags(station.Tags)
	if err != nil {
		return err
	}
	return r.q.UpdateStation(ctx, db.UpdateStationParams{
		Name:        station.Name,
		StreamUrl:   station.StreamURL,
		HomepageUrl: station.HomepageURL,
		FaviconUrl:  station.FaviconURL,
		Country:     station.Country,
		Language:    station.Language,
		Tags:        tags,
		Codec:       station.Codec,
		Bitrate:     int64(station.Bitrate),
		IsFavorite:  boolInt(station.IsFavorite),
		Position:    int64(station.Position),
		UserID:      userID,
		ID:          station.ID,
	})
}

func (r *Repository) Delete(ctx context.Context, userID, stationID string) error {
	affected, err := r.q.DeleteStation(ctx, db.DeleteStationParams{UserID: userID, ID: stationID})
	if err != nil {
		return err
	}
	if affected == 0 {
		return ErrNotFound
	}
	return nil
}

func (r *Repository) UpdateNowPlaying(ctx context.Context, stationID string, now NowPlaying) error {
	return r.q.UpdateNowPlaying(ctx, db.UpdateNowPlayingParams{
		LastNowPlayingTitle:     now.Title,
		LastNowPlayingArtist:    now.Artist,
		LastNowPlayingRaw:       now.Raw,
		LastNowPlayingUpdatedAt: now.UpdatedAt,
		ID:                      stationID,
	})
}

func stationOrNotFound(row db.RadioStation, err error) (Station, error) {
	if errors.Is(err, sql.ErrNoRows) {
		return Station{}, ErrNotFound
	}
	if err != nil {
		return Station{}, err
	}
	return toStation(row), nil
}

func toStation(row db.RadioStation) Station {
	station := Station{
		ID:          row.ID,
		Name:        row.Name,
		StreamURL:   row.StreamUrl,
		HomepageURL: row.HomepageUrl,
		FaviconURL:  row.FaviconUrl,
		Country:     row.Country,
		Language:    row.Language,
		Tags:        decodeTags(row.Tags),
		Codec:       row.Codec,
		Bitrate:     int(row.Bitrate),
		Source:      row.Source,
		ExternalID:  row.ExternalID,
		IsFavorite:  row.IsFavorite != 0,
		Position:    int(row.Position),
	}
	if row.LastNowPlayingTitle != "" || row.LastNowPlayingArtist != "" || row.LastNowPlayingRaw != "" || row.LastNowPlayingUpdatedAt != nil {
		station.LastNowPlaying = &NowPlaying{
			Title:     row.LastNowPlayingTitle,
			Artist:    row.LastNowPlayingArtist,
			Raw:       row.LastNowPlayingRaw,
			UpdatedAt: row.LastNowPlayingUpdatedAt,
		}
	}
	return station
}

func encodeTags(tags []string) (string, error) {
	if tags == nil {
		tags = []string{}
	}
	data, err := json.Marshal(tags)
	if err != nil {
		return "", err
	}
	return string(data), nil
}

func decodeTags(raw string) []string {
	var tags []string
	if err := json.Unmarshal([]byte(raw), &tags); err != nil {
		return []string{}
	}
	return tags
}

func boolInt(value bool) int64 {
	if value {
		return 1
	}
	return 0
}
