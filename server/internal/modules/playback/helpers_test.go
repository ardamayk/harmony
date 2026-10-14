package playback

import (
	"database/sql"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

func newTestService(sqlDB *sql.DB, tracks library.TrackReader) *Service {
	return NewService(NewRepository(sqlDB), tracks)
}
