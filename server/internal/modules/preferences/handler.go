package preferences

import (
	"encoding/json"
	"net/http"

	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
	"github.com/ardam/navidrome-replacement/server/internal/auth"
)

type Handler struct {
	service *Service
}

func NewHandler(service *Service) *Handler {
	return &Handler{service: service}
}

func (h *Handler) Get(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	prefs, err := h.service.Get(r.Context(), userID)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, prefs)
}

func (h *Handler) Patch(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	var patch UserPreferencesPatch
	if decodeErr := json.NewDecoder(r.Body).Decode(&patch); decodeErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", "invalid JSON body")
		return
	}
	if validationErr := ValidatePlaybackPatch(patch.Playback); validationErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", validationErr.Error())
		return
	}

	prefs, err := h.service.Patch(r.Context(), userID, patch)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	respond.JSON(w, http.StatusOK, prefs)
}
