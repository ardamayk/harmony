package playback

import (
	"encoding/json"
	"errors"
	"net/http"

	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/go-chi/chi/v5"
)

type Handler struct {
	service     *Service
	tracks      library.TrackAccess
	queueEvents *QueueEventBroker
}

func NewHandler(service *Service, tracks library.TrackAccess, queueEvents *QueueEventBroker) *Handler {
	return &Handler{service: service, tracks: tracks, queueEvents: queueEvents}
}

func (h *Handler) GetQueue(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	queue, err := h.service.GetQueue(r.Context(), userID)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, queue)
}

func (h *Handler) ReplaceQueue(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	var body struct {
		TrackIDs []string        `json:"trackIds"`
		Source   json.RawMessage `json:"source"`
		Revision string          `json:"revision"`
	}
	if decodeErr := decodeQueueRequest(w, r, &body); decodeErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", decodeErr.Error())
		return
	}

	if body.Revision == "" {
		respond.Error(w, http.StatusBadRequest, "bad_request", "revision is required")
		return
	}

	source, err := parseQueueItemSource(body.Source)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}

	queue, err := h.service.ReplaceQueue(r.Context(), userID, body.TrackIDs, body.Revision, source)
	if errors.Is(err, ErrRevisionConflict) {
		h.queueConflict(w, r, userID)
		return
	}
	if errors.Is(err, ErrQueueLimitExceeded) || errors.Is(err, ErrInvalidQueueSource) {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	if errors.Is(err, library.ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "track not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	h.respondQueueMutation(w, userID, queue)
}

func (h *Handler) AppendItem(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	var body struct {
		TrackID  string          `json:"trackId"`
		Source   json.RawMessage `json:"source"`
		Revision string          `json:"revision"`
	}
	if decodeErr := decodeQueueRequest(w, r, &body); decodeErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", decodeErr.Error())
		return
	}
	if body.TrackID == "" {
		respond.Error(w, http.StatusBadRequest, "bad_request", "trackId is required")
		return
	}
	if body.Revision == "" {
		respond.Error(w, http.StatusBadRequest, "bad_request", "revision is required")
		return
	}

	source, err := parseQueueItemSource(body.Source)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}

	queue, err := h.service.AppendItem(r.Context(), userID, body.TrackID, body.Revision, source)
	if errors.Is(err, ErrRevisionConflict) {
		h.queueConflict(w, r, userID)
		return
	}
	if errors.Is(err, ErrQueueLimitExceeded) || errors.Is(err, ErrInvalidQueueSource) {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	if errors.Is(err, library.ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "track not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	h.respondQueueMutation(w, userID, queue)
}

func (h *Handler) RemoveItem(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	itemID := chi.URLParam(r, "itemId")
	revision := r.Header.Get("If-Match")
	if revision == "" {
		respond.Error(w, http.StatusBadRequest, "bad_request", "If-Match revision is required")
		return
	}
	queue, err := h.service.RemoveItem(r.Context(), userID, itemID, revision)
	if errors.Is(err, ErrRevisionConflict) {
		h.queueConflict(w, r, userID)
		return
	}
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "queue item not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	h.respondQueueMutation(w, userID, queue)
}

func (h *Handler) ClearQueue(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	revision := r.Header.Get("If-Match")
	if revision == "" {
		respond.Error(w, http.StatusBadRequest, "bad_request", "If-Match revision is required")
		return
	}
	queue, err := h.service.ClearQueue(r.Context(), userID, r.URL.Query().Get("afterItemId"), revision)
	if errors.Is(err, ErrRevisionConflict) {
		h.queueConflict(w, r, userID)
		return
	}
	if errors.Is(err, ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "queue item not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	h.respondQueueMutation(w, userID, queue)
}

func (h *Handler) ReorderQueue(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	var body struct {
		ItemIDs  []string `json:"itemIds"`
		Revision string   `json:"revision"`
	}
	if decodeErr := decodeQueueRequest(w, r, &body); decodeErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", decodeErr.Error())
		return
	}
	if body.ItemIDs == nil || body.Revision == "" {
		respond.Error(w, http.StatusBadRequest, "bad_request", "itemIds and revision are required")
		return
	}
	queue, err := h.service.ReorderItems(r.Context(), userID, body.ItemIDs, body.Revision)
	if errors.Is(err, ErrRevisionConflict) {
		h.queueConflict(w, r, userID)
		return
	}
	if errors.Is(err, ErrInvalidQueueOrder) {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	h.respondQueueMutation(w, userID, queue)
}

func (h *Handler) respondQueueMutation(w http.ResponseWriter, userID string, queue Queue) {
	h.queueEvents.Publish(userID, queue.Revision, queue.EventSequence)
	respond.JSON(w, http.StatusOK, queue)
}

func (h *Handler) queueConflict(w http.ResponseWriter, r *http.Request, userID string) {
	queue, err := h.service.GetQueue(r.Context(), userID)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusConflict, struct {
		Error   string `json:"error"`
		Code    string `json:"code"`
		Message string `json:"message"`
		Queue   Queue  `json:"queue"`
	}{
		Error:   "conflict",
		Code:    "queue_revision_conflict",
		Message: "queue changed since supplied revision",
		Queue:   queue,
	})
}

func (h *Handler) StreamTrack(w http.ResponseWriter, r *http.Request) {
	trackID := chi.URLParam(r, "trackId")
	filePath, err := h.tracks.GetTrackFilePath(r.Context(), trackID)
	if errors.Is(err, library.ErrNotFound) {
		respond.Error(w, http.StatusNotFound, "not_found", "track not found")
		return
	}
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}

	if err := ServeTrackFile(w, r, filePath); err != nil {
		if errors.Is(err, ErrNotFound) {
			respond.Error(w, http.StatusNotFound, "not_found", "track file not found")
			return
		}
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
	}
}
