package radio

import (
	"errors"
	"time"
)

const ManualSource = "manual"

var ErrNotFound = errors.New("radio station not found")

type NowPlaying struct {
	Title     string     `json:"title,omitempty"`
	Artist    string     `json:"artist,omitempty"`
	Raw       string     `json:"raw,omitempty"`
	UpdatedAt *time.Time `json:"updatedAt,omitempty"`
	Stale     bool       `json:"stale"`
}

type Station struct {
	ID             string      `json:"id"`
	Name           string      `json:"name"`
	StreamURL      string      `json:"streamUrl"`
	HomepageURL    string      `json:"homepageUrl,omitempty"`
	FaviconURL     string      `json:"faviconUrl,omitempty"`
	Country        string      `json:"country,omitempty"`
	Language       string      `json:"language,omitempty"`
	Tags           []string    `json:"tags"`
	Codec          string      `json:"codec,omitempty"`
	Bitrate        int         `json:"bitrate,omitempty"`
	Source         string      `json:"source"`
	ExternalID     string      `json:"externalId,omitempty"`
	IsFavorite     bool        `json:"isFavorite"`
	Position       int         `json:"position"`
	LastNowPlaying *NowPlaying `json:"lastNowPlaying,omitempty"`
}

type StationList struct {
	Items []Station `json:"items"`
	Total int       `json:"total"`
}

type StationCreate struct {
	Name        string   `json:"name"`
	StreamURL   string   `json:"streamUrl"`
	HomepageURL string   `json:"homepageUrl,omitempty"`
	FaviconURL  string   `json:"faviconUrl,omitempty"`
	Country     string   `json:"country,omitempty"`
	Language    string   `json:"language,omitempty"`
	Tags        []string `json:"tags,omitempty"`
	Codec       string   `json:"codec,omitempty"`
	Bitrate     int      `json:"bitrate,omitempty"`
	Source      string   `json:"source,omitempty"`
	ExternalID  string   `json:"externalId,omitempty"`
	IsFavorite  bool     `json:"isFavorite"`
}

type StationPatch struct {
	Name        *string  `json:"name,omitempty"`
	StreamURL   *string  `json:"streamUrl,omitempty"`
	HomepageURL *string  `json:"homepageUrl,omitempty"`
	FaviconURL  *string  `json:"faviconUrl,omitempty"`
	Country     *string  `json:"country,omitempty"`
	Language    *string  `json:"language,omitempty"`
	Tags        []string `json:"tags,omitempty"`
	Codec       *string  `json:"codec,omitempty"`
	Bitrate     *int     `json:"bitrate,omitempty"`
	IsFavorite  *bool    `json:"isFavorite,omitempty"`
	Position    *int     `json:"position,omitempty"`
}
