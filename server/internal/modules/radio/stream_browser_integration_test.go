package radio

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

const browserIntegrationEnvironment = "RUN_HLS_BROWSER_INTEGRATION"

const fixtureUpstreamOrigin = "http://hls-fixture.example"

// TestBrowserHLSProxyIntegration runs with:
// RUN_HLS_BROWSER_INTEGRATION=1 go test ./internal/modules/radio -run TestBrowserHLSProxyIntegration -count=1 -v
func TestBrowserHLSProxyIntegration(t *testing.T) {
	if os.Getenv(browserIntegrationEnvironment) != "1" {
		t.Skip("set RUN_HLS_BROWSER_INTEGRATION=1 to run real browser integration")
	}

	var upstreamPaths []string
	var upstreamMu sync.Mutex
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		upstreamMu.Lock()
		upstreamPaths = append(upstreamPaths, r.URL.Path)
		upstreamMu.Unlock()
		serveHLSFixture(w, r)
	}))
	defer upstream.Close()

	store := newTestService(testutil.OpenMigratedDB(t))
	station, err := store.CreateStation(context.Background(), auth.DefaultUserID, StationCreate{
		Name:      "Browser HLS Fixture",
		StreamURL: fixtureUpstreamOrigin + "/master.m3u8",
	})
	if err != nil {
		t.Fatal(err)
	}
	cache := NewNowPlayingCache()
	proxy := NewStreamProxy(store, cache)
	proxy.client.Transport = fixtureUpstreamTransport(t, upstream.URL)
	handlers := NewHandler(store, nil, proxy, cache)
	router := chi.NewRouter()
	router.Use(hlsFixtureCORS)
	router.Get("/api/v1/radio/stations/{stationId}/stream", handlers.StreamStation)
	musicServer := httptest.NewServer(router)
	defer musicServer.Close()

	// CI runs Playwright with one retry, so the deadline must cover two
	// attempts plus browser startup and report flushing before diagnostics
	// are written.
	commandContext, cancel := context.WithTimeout(context.Background(), 180*time.Second)
	defer cancel()
	command := exec.CommandContext(commandContext, "bun", "x", "playwright", "test", "e2e/radio-hls-proxy.spec.ts", "--config=playwright.hls.config.ts")
	command.Dir = filepath.Clean(filepath.Join("..", "..", "..", "..", "web"))
	command.Env = append(os.Environ(),
		"HLS_PROXY_URL="+musicServer.URL+"/api/v1/radio/stations/"+station.ID+"/stream",
		"HLS_UPSTREAM_ORIGIN="+fixtureUpstreamOrigin,
		"PLAYWRIGHT_HTML_OPEN=never",
	)
	output, err := command.CombinedOutput()
	t.Logf("Playwright output:\n%s", output)
	if err != nil {
		t.Fatalf("run Playwright HLS integration: %v", err)
	}

	upstreamMu.Lock()
	defer upstreamMu.Unlock()
	for _, expectedPath := range []string{"/master.m3u8", "/media/index.m3u8", "/media/segment.ts"} {
		if !slices.Contains(upstreamPaths, expectedPath) {
			t.Fatalf("upstream paths = %q, missing %q", upstreamPaths, expectedPath)
		}
	}
}

func fixtureUpstreamTransport(t *testing.T, upstreamURL string) http.RoundTripper {
	t.Helper()
	target, err := url.Parse(upstreamURL)
	if err != nil {
		t.Fatal(err)
	}
	client := &http.Client{Transport: http.DefaultTransport.(*http.Transport).Clone()}
	return roundTripFunc(func(request *http.Request) (*http.Response, error) {
		forwarded := request.Clone(request.Context())
		forwarded.URL = new(url.URL)
		*forwarded.URL = *request.URL
		forwarded.URL.Scheme = target.Scheme
		forwarded.URL.Host = target.Host
		response, err := client.Do(forwarded)
		if err != nil {
			return nil, err
		}
		response.Request = request
		return response, nil
	})
}

func serveHLSFixture(w http.ResponseWriter, r *http.Request) {
	switch r.URL.Path {
	case "/master.m3u8":
		w.Header().Set("Content-Type", "application/vnd.apple.mpegurl")
		_, _ = io.WriteString(w, "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000,CODECS=\"mp4a.40.2\"\nmedia/index.m3u8\n")
	case "/media/index.m3u8":
		w.Header().Set("Content-Type", "application/vnd.apple.mpegurl")
		_, _ = io.WriteString(w, "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:6.0,\nsegment.ts\n#EXT-X-ENDLIST\n")
	case "/media/segment.ts":
		w.Header().Set("Content-Type", "video/mp2t")
		packet := make([]byte, 188)
		packet[0] = 0x47
		_, _ = w.Write(packet)
	default:
		http.Error(w, fmt.Sprintf("unknown fixture path %q", r.URL.Path), http.StatusNotFound)
	}
}

func hlsFixtureCORS(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Range")
		w.Header().Set("Access-Control-Expose-Headers", "Content-Range, Content-Type")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}
