package library

import (
	"context"
	"errors"
	"net/http"
	"strconv"

	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
	"github.com/go-chi/chi/v5"
)

type Handler struct {
	service *Service
	// nil until the module enables waveforms; the handler then answers 503.
	waveforms *WaveformService
}

func NewHandler(service *Service) *Handler {
	return &Handler{service: service}
}

// GetTrackWaveform answers cached peaks, 202 while they are being generated,
// 404 for unknown tracks or missing files, and 503 without ffmpeg.
func (h *Handler) GetTrackWaveform(w http.ResponseWriter, r *http.Request) {
	trackID := chi.URLParam(r, "trackId")
	waveform, err := h.waveforms.Get(r.Context(), trackID)
	switch {
	case err == nil:
		w.Header().Set("Cache-Control", "private, no-cache")
		respond.JSON(w, http.StatusOK, waveform)
	case errors.Is(err, ErrWaveformPending):
		w.Header().Set("Retry-After", "2")
		respond.JSON(w, http.StatusAccepted, TrackWaveformPending{Status: "pending", RetryAfterSeconds: 2})
	case errors.Is(err, ErrNotFound):
		respond.Error(w, http.StatusNotFound, "not_found", "track or track file not found")
	case errors.Is(err, ErrWaveformUnavailable):
		respond.Error(w, http.StatusServiceUnavailable, "dependency_unavailable", "waveforms need ffmpeg on the Music Server")
	case errors.Is(err, context.Canceled):
		return
	default:
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
	}
}

func (h *Handler) ListArtists(w http.ResponseWriter, r *http.Request) {
	limit, offset := pagination(r)
	q := r.URL.Query().Get("q")
	result, err := h.service.ListArtists(r.Context(), limit, offset, q)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, result)
}

func (h *Handler) ListAlbums(w http.ResponseWriter, r *http.Request) {
	limit, offset := pagination(r)
	q := r.URL.Query().Get("q")
	artistID := r.URL.Query().Get("artistId")
	result, err := h.service.ListAlbums(r.Context(), limit, offset, artistID, q)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, result)
}

func (h *Handler) GetAlbumCover(w http.ResponseWriter, r *http.Request) {
	albumID := chi.URLParam(r, "albumId")
	mime, data, err := h.service.GetAlbumCover(r.Context(), albumID)
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "album cover not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	w.Header().Set("Content-Type", mime)
	w.Header().Set("Cache-Control", "public, max-age=86400")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(data)
}

func (h *Handler) GetAlbum(w http.ResponseWriter, r *http.Request) {
	albumID := chi.URLParam(r, "albumId")
	result, err := h.service.GetAlbum(r.Context(), albumID)
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "album not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, result)
}

func (h *Handler) ListTracks(w http.ResponseWriter, r *http.Request) {
	limit, offset := pagination(r)
	q := r.URL.Query().Get("q")
	result, err := h.service.ListTracks(r.Context(), limit, offset, r.URL.Query().Get("artistId"), q)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, result)
}

func (h *Handler) GetTrack(w http.ResponseWriter, r *http.Request) {
	trackID := chi.URLParam(r, "trackId")
	result, err := h.service.GetTrack(r.Context(), trackID)
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "track not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, result)
}

func (h *Handler) GetTrackLyrics(w http.ResponseWriter, r *http.Request) {
	trackID := chi.URLParam(r, "trackId")
	result, err := h.service.GetTrackLyrics(r.Context(), trackID)
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "track not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, result)
}

func pagination(r *http.Request) (limit, offset int) {
	limit = 50
	offset = 0
	if v := r.URL.Query().Get("limit"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 && n <= 500 {
			limit = n
		}
	}
	if v := r.URL.Query().Get("offset"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n >= 0 {
			offset = n
		}
	}
	return limit, offset
}
