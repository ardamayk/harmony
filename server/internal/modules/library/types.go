package library

import "time"

type Artist struct {
	ID         string `json:"id"`
	Name       string `json:"name"`
	AlbumCount int    `json:"albumCount,omitempty"`
}

type ArtistCredit struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type Genre struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type ReleaseIdentifier struct {
	Scheme string `json:"scheme"`
	Value  string `json:"value"`
}

type AlbumArtworkMetadata struct {
	SourceTrackID string `json:"sourceTrackId,omitempty"`
	ContentSHA256 string `json:"contentSha256"`
	MediaType     string `json:"mediaType,omitempty"`
	Width         int    `json:"width,omitempty"`
	Height        int    `json:"height,omitempty"`
	SizeBytes     int64  `json:"sizeBytes"`
}

type Album struct {
	ID                 string                `json:"id"`
	Title              string                `json:"title"`
	ArtistID           string                `json:"artistId"`
	ArtistName         string                `json:"artistName"`
	AlbumArtists       []ArtistCredit        `json:"albumArtists"`
	Year               *int                  `json:"year,omitempty"`
	ReleaseDate        *string               `json:"releaseDate,omitempty"`
	ReleaseIdentifiers []ReleaseIdentifier   `json:"releaseIdentifiers"`
	TrackCount         int                   `json:"trackCount,omitempty"`
	Genres             []string              `json:"genres,omitempty"`
	GenreItems         []Genre               `json:"genreItems"`
	Artwork            *AlbumArtworkMetadata `json:"artwork,omitempty"`
}

// TrackLyrics is the payload of GET /api/v1/library/tracks/{trackId}/lyrics.
type TrackLyrics struct {
	TrackID string `json:"trackId"`
	Lyrics  string `json:"lyrics"`
}

type Track struct {
	ID           string             `json:"id"`
	Title        string             `json:"title"`
	ArtistName   string             `json:"artistName"`
	Artists      []ArtistCredit     `json:"artists"`
	AlbumArtists []ArtistCredit     `json:"albumArtists"`
	AlbumID      string             `json:"albumId"`
	AlbumTitle   string             `json:"albumTitle,omitempty"`
	DiscNo       int                `json:"discNo"`
	TrackNo      *int               `json:"trackNo,omitempty"`
	TrackTotal   *int               `json:"trackTotal,omitempty"`
	DiscTotal    *int               `json:"discTotal,omitempty"`
	DurationMs   int                `json:"durationMs"`
	Format       string             `json:"format"`
	Codec        string             `json:"codec,omitempty"`
	Container    string             `json:"container,omitempty"`
	SampleFormat string             `json:"sampleFormat,omitempty"`
	SizeBytes    int64              `json:"sizeBytes,omitempty"`
	Genre        string             `json:"genre,omitempty"`
	Genres       []Genre            `json:"genres"`
	SampleRateHz int                `json:"sampleRateHz,omitempty"`
	BitDepth     int                `json:"bitDepth,omitempty"`
	ChannelCount int                `json:"channelCount,omitempty"`
	BitrateBps   int                `json:"bitrateBps,omitempty"`
	BitrateKbps  int                `json:"bitrateKbps,omitempty"`
	ReplayGain   ReplayGainMetadata `json:"replayGain"`
	// Every remaining stored column, so a detail page can show all the Music
	// Server knows about a Track (single source of truth: the tracks row).
	TitleSort     string    `json:"titleSort"`
	FilePath      string    `json:"filePath"`
	FileMtime     int64     `json:"fileMtime"`
	ContentSHA256 string    `json:"contentSha256,omitempty"`
	IdentityKey   string    `json:"identityKey,omitempty"`
	Revision      int       `json:"revision"`
	CreatedAt     time.Time `json:"createdAt"`
	UpdatedAt     time.Time `json:"updatedAt"`
}

type AlbumDetail struct {
	Album
	Tracks []Track `json:"tracks"`
}

type ArtistList struct {
	Items []Artist `json:"items"`
	Total int      `json:"total"`
}

type AlbumList struct {
	Items []Album `json:"items"`
	Total int     `json:"total"`
}

type TrackList struct {
	Items []Track `json:"items"`
	Total int     `json:"total"`
}
