package staticassets

import (
	"embed"
	"io/fs"
	"net/http"
)

//go:embed all:web/*
var webFS embed.FS

//go:embed docs/*
var docsFS embed.FS

func WebHandler() http.Handler {
	sub, err := fs.Sub(webFS, "web")
	if err != nil {
		return http.NotFoundHandler()
	}
	return spaHandler(http.FileServer(http.FS(sub)), sub)
}

func DocsHandler() http.Handler {
	sub, err := fs.Sub(docsFS, "docs")
	if err != nil {
		return http.NotFoundHandler()
	}
	return http.StripPrefix("/docs", http.FileServer(http.FS(sub)))
}

func spaHandler(fileServer http.Handler, assets fs.FS) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/" {
			if _, err := fs.Stat(assets, r.URL.Path[1:]); err == nil {
				fileServer.ServeHTTP(w, r)
				return
			}
		}
		data, err := fs.ReadFile(assets, "index.html")
		if err != nil {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		_, _ = w.Write(data)
	})
}
