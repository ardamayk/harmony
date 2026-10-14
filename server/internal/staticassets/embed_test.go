package staticassets

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestWebHandlerServesUnderscoreChunks(t *testing.T) {
	// Read the build output independently so omitted embedded files are detected.
	chunkPaths, err := filepath.Glob("web/assets/_*.js")
	if err != nil {
		t.Fatalf("find route chunks: %v", err)
	}
	if len(chunkPaths) == 0 {
		t.Fatal("expected underscore-prefixed route chunks in web/assets")
	}
	handler := WebHandler()
	for _, chunkPath := range chunkPaths {
		t.Run(filepath.Base(chunkPath), func(t *testing.T) {
			assertJavaScriptChunk(t, handler, chunkPath)
		})
	}
}

func assertJavaScriptChunk(t *testing.T, handler http.Handler, chunkPath string) {
	t.Helper()
	expected, err := os.ReadFile(chunkPath)
	if err != nil {
		t.Fatalf("read route chunk %q: %v", chunkPath, err)
	}
	requestPath := "/" + strings.TrimPrefix(filepath.ToSlash(chunkPath), "web/")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, httptest.NewRequest(http.MethodGet, requestPath, nil))
	if response.Code != http.StatusOK {
		t.Fatalf("GET %s: status = %d, want 200", requestPath, response.Code)
	}
	if contentType := response.Header().Get("Content-Type"); !strings.Contains(contentType, "javascript") {
		t.Fatalf("GET %s: Content-Type = %q, want JavaScript", requestPath, contentType)
	}
	if !bytes.Equal(response.Body.Bytes(), expected) {
		t.Fatalf("GET %s: response does not match the built route chunk", requestPath)
	}
}
