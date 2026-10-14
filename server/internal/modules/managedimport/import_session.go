package managedimport

import (
	"context"
	"net/http"

	"github.com/go-chi/chi/v5"
)

func (handlers *Handlers) HeartbeatBatch(writer http.ResponseWriter, request *http.Request) {
	if err := handlers.service.HeartbeatBatch(request.Context(), chi.URLParam(request, "batchId")); err != nil {
		handleError(writer, request, err)
		return
	}
	writer.WriteHeader(http.StatusNoContent)
}

func (service *Service) HeartbeatBatch(ctx context.Context, batchID string) error {
	// Serialize against cancellation so a fresh heartbeat wins before cleanup
	// rechecks eligibility. Liveness never changes the preview revision.
	managedImportBatchConfirmationMu.Lock()
	defer managedImportBatchConfirmationMu.Unlock()
	return service.repo.HeartbeatBatch(ctx, batchID)
}
