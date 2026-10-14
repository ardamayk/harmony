package radio

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"

	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/go-chi/chi/v5"
)

type Searcher interface {
	Search(r *http.Request) (SearchResultList, error)
	LookupStation(r *http.Request, stationUUID string) (SearchResult, error)
	Countries(ctx context.Context) (CatalogOptionList, error)
	Tags(ctx context.Context) (CatalogOptionList, error)
}

type Streamer interface {
	Stream(w http.ResponseWriter, r *http.Request, station Station)
}

type Handler struct {
	service  *Service
	searcher Searcher
	streamer Streamer
	cache    *NowPlayingCache
}

func NewHandler(service *Service, searcher Searcher, streamer Streamer, cache ...*NowPlayingCache) *Handler {
	nowPlayingCache := NewNowPlayingCache()
	if len(cache) > 0 && cache[0] != nil {
		nowPlayingCache = cache[0]
	}
	return &Handler{service: service, searcher: searcher, streamer: streamer, cache: nowPlayingCache}
}

func (h *Handler) ListStations(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	list, err := h.service.ListStations(r.Context(), userID)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, list)
}

func (h *Handler) CreateStation(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	var body StationCreate
	if decodeErr := json.NewDecoder(r.Body).Decode(&body); decodeErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", "invalid JSON body")
		return
	}
	station, err := h.service.CreateStation(r.Context(), userID, body)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	respond.JSON(w, http.StatusCreated, station)
}

func (h *Handler) GetStation(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	station, err := h.service.GetStation(r.Context(), userID, chi.URLParam(r, "stationId"))
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "radio station not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, station)
}

func (h *Handler) PatchStation(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	var body StationPatch
	if decodeErr := json.NewDecoder(r.Body).Decode(&body); decodeErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", "invalid JSON body")
		return
	}
	station, err := h.service.UpdateStation(r.Context(), userID, chi.URLParam(r, "stationId"), body)
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "radio station not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, station)
}

func (h *Handler) DeleteStation(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	err = h.service.DeleteStation(r.Context(), userID, chi.URLParam(r, "stationId"))
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "radio station not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (h *Handler) SearchStations(w http.ResponseWriter, r *http.Request) {
	if h.searcher == nil {
		respond.Error(w, http.StatusServiceUnavailable, "unavailable", "radio search unavailable")
		return
	}
	results, err := h.searcher.Search(r)
	if err != nil {
		respond.Error(w, http.StatusBadGateway, "bad_gateway", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, results)
}

func (h *Handler) ListCatalogCountries(w http.ResponseWriter, r *http.Request) {
	if h.searcher == nil {
		respond.Error(w, http.StatusServiceUnavailable, "unavailable", "radio search unavailable")
		return
	}
	results, err := h.searcher.Countries(r.Context())
	if err != nil {
		respond.Error(w, http.StatusBadGateway, "bad_gateway", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, results)
}

func (h *Handler) ListCatalogTags(w http.ResponseWriter, r *http.Request) {
	if h.searcher == nil {
		respond.Error(w, http.StatusServiceUnavailable, "unavailable", "radio search unavailable")
		return
	}
	results, err := h.searcher.Tags(r.Context())
	if err != nil {
		respond.Error(w, http.StatusBadGateway, "bad_gateway", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, results)
}

type importRequest struct {
	StationUUID string        `json:"stationUuid"`
	Result      *SearchResult `json:"result,omitempty"`
}

func (h *Handler) ImportStation(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	var body importRequest
	if decodeErr := json.NewDecoder(r.Body).Decode(&body); decodeErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", "invalid JSON body")
		return
	}
	result, err := h.importResult(r, body)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	station, err := h.service.ImportStation(r.Context(), userID, result)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	respond.JSON(w, http.StatusCreated, station)
}

func (h *Handler) PreviewStation(w http.ResponseWriter, r *http.Request) {
	if h.streamer == nil {
		respond.Error(w, http.StatusServiceUnavailable, "unavailable", "radio streaming unavailable")
		return
	}
	var body importRequest
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", "invalid JSON body")
		return
	}
	result, err := h.importResult(r, body)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	h.streamer.Stream(w, r, previewStation(result))
}

func (h *Handler) StreamPreviewStation(w http.ResponseWriter, r *http.Request) {
	if h.streamer == nil {
		respond.Error(w, http.StatusServiceUnavailable, "unavailable", "radio streaming unavailable")
		return
	}
	if h.searcher == nil {
		respond.Error(w, http.StatusServiceUnavailable, "unavailable", "radio search unavailable")
		return
	}
	stationUUID := chi.URLParam(r, "stationUuid")
	result, err := h.searcher.LookupStation(r, stationUUID)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	h.streamer.Stream(w, r, previewStation(result))
}

func (h *Handler) importResult(r *http.Request, body importRequest) (SearchResult, error) {
	if body.StationUUID != "" && h.searcher != nil {
		result, err := h.searcher.LookupStation(r, body.StationUUID)
		if err == nil {
			return result, nil
		}
		if body.Result == nil {
			return SearchResult{}, err
		}
	}
	if body.Result != nil {
		return *body.Result, nil
	}
	return SearchResult{}, errors.New("stationUuid or result is required")
}

func previewStation(result SearchResult) Station {
	return Station{
		ID:          "preview:" + result.StationUUID,
		Name:        result.Name,
		StreamURL:   result.StreamURL,
		HomepageURL: result.HomepageURL,
		FaviconURL:  result.FaviconURL,
		Country:     result.Country,
		Language:    result.Language,
		Tags:        result.Tags,
		Codec:       result.Codec,
		Bitrate:     result.Bitrate,
		Source:      RadioBrowserSource,
		ExternalID:  result.StationUUID,
	}
}

func (h *Handler) StreamStation(w http.ResponseWriter, r *http.Request) {
	if h.streamer == nil {
		respond.Error(w, http.StatusServiceUnavailable, "unavailable", "radio streaming unavailable")
		return
	}
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	station, err := h.service.GetStation(r.Context(), userID, chi.URLParam(r, "stationId"))
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "radio station not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	h.streamer.Stream(w, r, station)
}

func (h *Handler) GetNowPlaying(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	stationID := chi.URLParam(r, "stationId")
	if strings.HasPrefix(stationID, "preview:") {
		if now, ok := h.cache.Get(stationID); ok {
			respond.JSON(w, http.StatusOK, now)
			return
		}
		respond.JSON(w, http.StatusOK, NowPlaying{})
		return
	}
	if _, stationErr := h.service.GetStation(r.Context(), userID, stationID); errors.Is(stationErr, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "radio station not found")
		return
	} else if stationErr != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", stationErr.Error())
		return
	}
	if now, ok := h.cache.Get(stationID); ok {
		respond.JSON(w, http.StatusOK, now)
		return
	}
	station, err := h.service.GetStation(r.Context(), userID, stationID)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	if station.LastNowPlaying != nil {
		respond.JSON(w, http.StatusOK, station.LastNowPlaying)
		return
	}
	respond.JSON(w, http.StatusOK, NowPlaying{})
}
