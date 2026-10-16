package library

import (
	"bytes"
	"context"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/go-chi/chi/v5"
)

func encodedTestCover(t *testing.T, width, height int, shade uint8) []byte {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, width, height))
	for y := range height {
		for x := range width {
			img.Set(x, y, color.RGBA{R: shade, G: 90, B: 160, A: 255})
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

func requestCover(t *testing.T, handlers *Handler, albumID, size string) *httptest.ResponseRecorder {
	t.Helper()
	target := "/"
	if size != "" {
		target = "/?size=" + size
	}
	request := httptest.NewRequest(http.MethodGet, target, nil)
	routeContext := chi.NewRouteContext()
	routeContext.URLParams.Add("albumId", albumID)
	request = request.WithContext(context.WithValue(request.Context(), chi.RouteCtxKey, routeContext))
	response := httptest.NewRecorder()
	handlers.GetAlbumCover(response, request)
	return response
}

func TestHandlersGetAlbumCoverScalesDownAndCachesBySource(t *testing.T) {
	handlers, database := setupHandlerFixture(t)
	albumID, trackID := seedTrack(t, database)
	artworkPath := filepath.Join(t.TempDir(), "cover.png")
	if err := os.WriteFile(artworkPath, encodedTestCover(t, 1200, 800, 40), 0o600); err != nil {
		t.Fatal(err)
	}
	executeFixtureStatement(t, database, `
		INSERT INTO album_artwork (
			id, album_id, source_track_id, content_sha256, media_type, width, height,
			encoded_size_bytes, file_path
		) VALUES ('artwork-file', ?, ?, '1111111111111111111111111111111111111111111111111111111111111111', 'image/png', 1200, 800, 1, ?)`,
		albumID, trackID, artworkPath)

	response := requestCover(t, handlers, albumID, "256")
	if response.Code != http.StatusOK || response.Header().Get("Content-Type") != "image/jpeg" {
		t.Fatalf("scaled cover = %d %q", response.Code, response.Header().Get("Content-Type"))
	}
	scaled, err := jpeg.DecodeConfig(bytes.NewReader(response.Body.Bytes()))
	if err != nil {
		t.Fatal(err)
	}
	if scaled.Width != 256 || scaled.Height != 170 {
		t.Fatalf("scaled cover = %dx%d, want 256x170 (fit, aspect kept)", scaled.Width, scaled.Height)
	}
	var stored int
	if err := database.QueryRow(`SELECT COUNT(*) FROM album_cover_variants WHERE album_id = ? AND size = 256 AND source_sha256 = '1111111111111111111111111111111111111111111111111111111111111111'`, albumID).Scan(&stored); err != nil || stored != 1 {
		t.Fatalf("stored variants = %d (%v), want 1", stored, err)
	}

	// A replaced cover has a new hash, so the stale variant is regenerated.
	if err := os.WriteFile(artworkPath, encodedTestCover(t, 600, 900, 200), 0o600); err != nil {
		t.Fatal(err)
	}
	executeFixtureStatement(t, database, `UPDATE album_artwork SET content_sha256 = '2222222222222222222222222222222222222222222222222222222222222222' WHERE id = 'artwork-file'`)
	response = requestCover(t, handlers, albumID, "256")
	regenerated, err := jpeg.DecodeConfig(bytes.NewReader(response.Body.Bytes()))
	if err != nil {
		t.Fatal(err)
	}
	if regenerated.Width != 170 || regenerated.Height != 256 {
		t.Fatalf("regenerated cover = %dx%d, want 170x256", regenerated.Width, regenerated.Height)
	}

	// A cover already within the size is served as is, never scaled up.
	response = requestCover(t, handlers, albumID, "1024")
	if response.Header().Get("Content-Type") != "image/png" {
		t.Fatalf("small cover type = %q, want the original PNG", response.Header().Get("Content-Type"))
	}

	if response := requestCover(t, handlers, albumID, "300"); response.Code != http.StatusBadRequest {
		t.Fatalf("unsupported size status = %d, want 400", response.Code)
	}
}
