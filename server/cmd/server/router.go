package main

import (
	"database/sql"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/api"
	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/ardam/navidrome-replacement/server/internal/dependencies"
	"github.com/ardam/navidrome-replacement/server/internal/modules"
	docsmodule "github.com/ardam/navidrome-replacement/server/internal/modules/docs"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/librarysearch"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport"
	"github.com/ardam/navidrome-replacement/server/internal/modules/playback"
	"github.com/ardam/navidrome-replacement/server/internal/modules/playlists"
	"github.com/ardam/navidrome-replacement/server/internal/modules/preferences"
	"github.com/ardam/navidrome-replacement/server/internal/modules/radio"
	"github.com/ardam/navidrome-replacement/server/internal/staticassets"
	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
)

const REQUEST_TIMEOUT = 60 * time.Second

// assembledServer is the complete versioned HTTP surface of the Music Server:
// every module mounted on one router exactly as the production binary serves
// it, so contract tests exercise the real route table.
type assembledServer struct {
	router       chi.Router
	registry     *modules.Registry
	importModule *managedimport.Module
}

func newAssembledServer(cfg config.Config, sqlDB *sql.DB, report dependencies.Report) assembledServer {
	prefModule := preferences.NewModule(sqlDB)
	libModule := library.NewModule(sqlDB)
	searchModule := librarysearch.NewModule(sqlDB)
	libModule.EnableWaveforms(report.Has(dependencies.FFMPEG))
	trackAccess := libModule.TrackAccess()
	queueEvents := playback.NewQueueEventBroker()
	importModule := managedimport.NewModule(sqlDB, cfg, library.NewMediaInspector(), queueEvents)
	playModule := playback.NewModule(sqlDB, trackAccess, queueEvents)
	playlistModule := playlists.NewModule(sqlDB, trackAccess)
	radioModule := radio.NewModule(sqlDB, cfg)
	docsModule := docsmodule.NewModule()
	apiHandler := api.NewHandler(cfg, report)

	registry := modules.NewRegistry(libModule, searchModule, importModule, playModule, playlistModule, radioModule, prefModule, docsModule)

	r := chi.NewRouter()
	r.Use(middleware.RequestID)
	r.Use(middleware.ClientIPFromRemoteAddr)
	r.Use(middleware.Recoverer)
	r.Use(streamAwareTimeout(REQUEST_TIMEOUT))
	r.Use(corsHandler(cfg.CORSOrigins))

	r.Get("/api/v1/health", apiHandler.GetHealth)
	r.Get("/api/v1/me", apiHandler.GetMe)
	registry.RegisterAll(r)

	r.Mount("/", staticassets.WebHandler())

	return assembledServer{router: r, registry: registry, importModule: importModule}
}
