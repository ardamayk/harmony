package managedimport

import (
	"errors"
	"io"
	"log/slog"
	"net/http"

	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/go-chi/chi/v5"
)

const MAX_UPLOADED_ARTWORK_BYTES int64 = 20 * 1024 * 1024

func (handlers *Handlers) UploadArtwork(writer http.ResponseWriter, request *http.Request) {
	data, err := io.ReadAll(http.MaxBytesReader(writer, request.Body, MAX_UPLOADED_ARTWORK_BYTES))
	if err != nil {
		respond.Error(writer, http.StatusBadRequest, "invalid_artwork", "Artwork must be JPEG or PNG and no larger than 20 MiB")
		return
	}
	artwork, err := library.ValidateUploadedArtwork(data)
	if err != nil {
		handleError(writer, request, validationError(err))
		return
	}
	managedImportBatchConfirmationMu.Lock()
	defer managedImportBatchConfirmationMu.Unlock()
	batchID, albumKey := chi.URLParam(request, "batchId"), chi.URLParam(request, "albumKey")
	batch, err := handlers.service.GetBatch(request.Context(), batchID)
	if err != nil {
		handleError(writer, request, err)
		return
	}
	if batch.Status != BATCH_STATUS_UPLOADING {
		handleError(writer, request, ErrInvalidState)
		return
	}
	hasAlbum := false
	for _, album := range batch.Albums {
		if album.Key == albumKey {
			hasAlbum = true
			break
		}
	}
	if !hasAlbum {
		handleError(writer, request, ErrNotFound)
		return
	}
	if err := handlers.service.storage.Preflight(StorageRequirement{TemporaryBytes: int64(len(data))}); err != nil {
		handleError(writer, request, err)
		return
	}
	id := "upload-" + albumKey
	if err := handlers.service.repo.saveArtwork(request.Context(), batchID, id, "", albumKey, artwork); err != nil {
		handleError(writer, request, err)
		return
	}
	respond.JSON(writer, http.StatusOK, ArtworkOption{ID: id, MediaType: artwork.MIMEType, ContentSHA256: artwork.SHA256})
}

func (handlers *Handlers) GetArtwork(writer http.ResponseWriter, request *http.Request) {
	artwork, err := handlers.service.repo.loadArtwork(request.Context(), chi.URLParam(request, "batchId"), chi.URLParam(request, "artworkId"))
	if err != nil {
		handleError(writer, request, err)
		return
	}
	writer.Header().Set("Content-Type", artwork.MIMEType)
	writer.Header().Set("X-Content-Type-Options", "nosniff")
	writer.Header().Set("Cache-Control", "no-store")
	if _, err := writer.Write(artwork.Data); err != nil && !errors.Is(err, request.Context().Err()) {
		slog.ErrorContext(request.Context(), "write import artwork response", "error", err)
	}
}
