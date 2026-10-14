package librarysearch

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"unicode/utf8"

	apigen "github.com/ardam/navidrome-replacement/server/internal/api/gen"
	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/ardam/navidrome-replacement/server/internal/searchdata"
)

const MAX_QUERY_CHARACTERS = 200

type Handler struct {
	service *Service
}

func NewHandler(service *Service) *Handler {
	return &Handler{service: service}
}

// Search validates the request and responds with ranked results.
func (h *Handler) Search(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	raw := r.URL.Query().Get("q")
	if strings.TrimSpace(raw) == "" {
		respond.Error(w, http.StatusBadRequest, "bad_request", "query parameter q is required")
		return
	}
	if utf8.RuneCountInString(raw) > MAX_QUERY_CHARACTERS {
		respond.Error(w, http.StatusBadRequest, "bad_request", fmt.Sprintf("query is longer than %d characters", MAX_QUERY_CHARACTERS))
		return
	}
	query := newQuery(searchdata.PrepareQuery(raw))
	if query.isEmpty() {
		respond.Error(w, http.StatusBadRequest, "bad_request", "query has no searchable letters or digits")
		return
	}
	all := false
	if values, present := r.URL.Query()["all"]; present {
		if len(values) != 1 || (values[0] != "true" && values[0] != "false") {
			respond.Error(w, http.StatusBadRequest, "bad_request", "query parameter all must be true or false")
			return
		}
		all = values[0] == "true"
	}
	results, err := h.service.Search(r.Context(), userID, query, all)
	if err != nil {
		if errors.Is(err, context.Canceled) {
			return
		}
		slog.Error("Library Search failed", "error", err)
		respond.Error(w, http.StatusInternalServerError, "internal_error", "Library Search is unavailable")
		return
	}
	respond.JSON(w, http.StatusOK, toResponse(results))
}

func toResponse(ranked rankedResults) apigen.LibrarySearchResponse {
	response := apigen.LibrarySearchResponse{
		Total:     &ranked.total,
		Tracks:    toResults(ranked.groups[KIND_TRACK]),
		Albums:    toResults(ranked.groups[KIND_ALBUM]),
		Artists:   toResults(ranked.groups[KIND_ARTIST]),
		Genres:    toResults(ranked.groups[KIND_GENRE]),
		Playlists: toResults(ranked.groups[KIND_PLAYLIST]),
	}
	if ranked.bestMatch != nil {
		best := toResult(*ranked.bestMatch)
		response.BestMatch = &best
	}
	return response
}

// creditFields names the field that carries each credited kind's displayed Artists.
var creditFields = map[string]string{"track": "track_artist", "album": "album_artist"}

func toResults(entries []listedEntry) []apigen.LibrarySearchResult {
	results := make([]apigen.LibrarySearchResult, 0, len(entries))
	for _, entry := range entries {
		results = append(results, toResult(entry))
	}
	return results
}

func toResult(entry listedEntry) apigen.LibrarySearchResult {
	item := entry.entity
	result := apigen.LibrarySearchResult{
		Type:  apigen.LibrarySearchResultType(item.kind),
		Id:    item.id,
		Name:  item.primary.Original,
		Match: entry.match,
	}
	if credits, credited := creditFields[item.kind]; credited {
		artists := make([]apigen.ArtistCredit, 0)
		for _, field := range item.fields(credits) {
			artists = append(artists, apigen.ArtistCredit{Id: field.relatedID, Name: field.text.Original})
		}
		result.Artists = &artists
	}
	if album := item.fields("album"); len(album) == 1 {
		result.Album = &apigen.LibrarySearchAlbumReference{Id: album[0].relatedID, Name: album[0].text.Original}
	}
	return result
}
