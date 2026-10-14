package managedimport

import (
	"context"
	"database/sql"
	"log/slog"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/go-chi/chi/v5"
)

type Module struct {
	handlers *Handlers
	service  *Service
}

func NewModule(database *sql.DB, configuration config.Config, inspector library.MediaInspector, queueEvents ...QueueInvalidationPublisher) *Module {
	return newModule(database, configuration, inspector, availableStorageBytes, queueEvents...)
}

func newModule(database *sql.DB, configuration config.Config, inspector library.MediaInspector, capacity storageCapacity, queueEvents ...QueueInvalidationPublisher) *Module {
	repo := NewRepository(database)
	fileLimit := configuration.ManagedImportFileLimitBytes
	if fileLimit <= 0 {
		fileLimit = config.DEFAULT_MANAGED_IMPORT_FILE_LIMIT_BYTES
	}
	batchLimit := configuration.ManagedImportBatchLimitBytes
	if batchLimit <= 0 {
		batchLimit = config.DEFAULT_MANAGED_IMPORT_BATCH_LIMIT_BYTES
	}
	storage := newStorage(configuration.ManagedStoragePath, StorageLimits{
		ReserveBytes: configuration.ManagedStorageReserveBytes,
		FileBytes:    fileLimit,
		BatchBytes:   batchLimit,
	}, capacity)
	service := NewService(repo, storage, inspector)
	if len(queueEvents) > 0 {
		service.queueEvents = queueEvents[0]
	}
	return &Module{handlers: NewHandlers(service), service: service}
}

func (module *Module) Name() string {
	return "managed-import"
}

func (module *Module) Start(ctx context.Context) error {
	if err := module.service.RecoverPendingTrackDeletions(ctx); err != nil {
		return err
	}
	if err := module.service.RecoverPendingTrackReplacements(ctx); err != nil {
		return err
	}
	if err := module.service.CleanupRestart(ctx); err != nil {
		return err
	}
	go module.cleanupInactive(ctx)
	return nil
}

func (module *Module) cleanupInactive(ctx context.Context) {
	ticker := time.NewTicker(IMPORT_CLEANUP_INTERVAL)
	defer ticker.Stop()
	for {
		select {
		case now := <-ticker.C:
			if err := module.service.CleanupInactive(ctx, now); err != nil {
				slog.ErrorContext(ctx, "inactive Managed Import cleanup failed", "error", err)
			}
		case <-ctx.Done():
			return
		}
	}
}

func (module *Module) RegisterRoutes(router chi.Router) {
	router.Get("/api/v1/library/albums/{albumId}/deletion", module.handlers.PreviewAlbumDeletion)
	router.Delete("/api/v1/library/albums/{albumId}", module.handlers.DeleteAlbum)
	router.Get("/api/v1/library/tracks/{trackId}/deletion", module.handlers.PreviewTrackDeletion)
	router.Delete("/api/v1/library/tracks/{trackId}", module.handlers.DeleteTrack)
	router.Post("/api/v1/library/tracks/{trackId}/replacement", module.handlers.CreateTrackReplacement)
	router.Get("/api/v1/import-history", module.handlers.ListHistory)
	router.Put("/api/v1/import-batches/{batchId}/albums/{albumKey}/artwork", module.handlers.UploadArtwork)
	router.Get("/api/v1/import-batches/{batchId}/artwork/{artworkId}", module.handlers.GetArtwork)
	router.Post("/api/v1/import-batches", module.handlers.CreateBatch)
	router.Get("/api/v1/import-batches/{batchId}", module.handlers.GetBatch)
	router.Post("/api/v1/import-batches/{batchId}/heartbeat", module.handlers.HeartbeatBatch)
	router.Delete("/api/v1/import-batches/{batchId}", module.handlers.CancelBatch)
	router.Post("/api/v1/import-batches/{batchId}/confirm", module.handlers.ConfirmBatch)
	router.Post("/api/v1/imports", module.handlers.CreateJob)
	router.Get("/api/v1/imports/{importId}", module.handlers.GetJob)
	router.Delete("/api/v1/imports/{importId}", module.handlers.CancelJob)
	router.Put("/api/v1/imports/{importId}/file", module.handlers.UploadFile)
	router.Post("/api/v1/imports/{importId}/confirm", module.handlers.Confirm)
	router.Post("/api/v1/imports/{importId}/replacement", module.handlers.ConfirmTrackReplacement)
}
