package playlists

import (
	"errors"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

const DefaultFavoritesName = "Favorites"

var ErrNotFound = errors.New("playlist not found")

type Playlist struct {
	ID         string `json:"id"`
	Name       string `json:"name"`
	IsDefault  bool   `json:"isDefault"`
	TrackCount int    `json:"trackCount"`
}

type PlaylistList struct {
	Items []Playlist `json:"items"`
	Total int        `json:"total"`
}

type PlaylistDetail struct {
	Playlist
	Tracks []library.Track `json:"tracks"`
}
