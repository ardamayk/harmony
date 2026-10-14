package library

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

// monoWAV writes a minimal 16-bit PCM RIFF file around the samples.
func monoWAV(sampleRate uint32, samples []int16) []byte {
	data := pcm(samples...)
	header := make([]byte, 44)
	copy(header[0:], "RIFF")
	binary.LittleEndian.PutUint32(header[4:], uint32(36+len(data)))
	copy(header[8:], "WAVE")
	copy(header[12:], "fmt ")
	binary.LittleEndian.PutUint32(header[16:], 16)
	binary.LittleEndian.PutUint16(header[20:], 1)
	binary.LittleEndian.PutUint16(header[22:], 1)
	binary.LittleEndian.PutUint32(header[24:], sampleRate)
	binary.LittleEndian.PutUint32(header[28:], sampleRate*2)
	binary.LittleEndian.PutUint16(header[32:], 2)
	binary.LittleEndian.PutUint16(header[34:], 16)
	copy(header[36:], "data")
	binary.LittleEndian.PutUint32(header[40:], uint32(len(data)))
	return append(header, data...)
}

func pcm(samples ...int16) []byte {
	out := make([]byte, len(samples)*2)
	for index, sample := range samples {
		binary.LittleEndian.PutUint16(out[index*2:], uint16(sample))
	}
	return out
}

func TestBucketPeaksKeepsTheLoudestSamplePerBinAndNormalizes(t *testing.T) {
	// Eight samples into four bins: [100, -200] [50, 0] [0, 0] [400, -300]
	peaks := bucketPeaks(pcm(100, -200, 50, 0, 0, 0, 400, -300), 4, 8)
	want := []byte{127, 31, 0, 255}
	for index := range want {
		if peaks[index] != want[index] {
			t.Fatalf("peaks = %v, want %v", peaks, want)
		}
	}
}

func TestBucketPeaksHandlesSilenceUnknownLengthAndOverrun(t *testing.T) {
	silence := bucketPeaks(pcm(0, 0, 0, 0), 2, 4)
	if silence[0] != 0 || silence[1] != 0 {
		t.Fatalf("silence = %v, want zeros", silence)
	}
	// Unknown total length buffers and buckets at the end.
	unknown := bucketPeaks(pcm(10, 20, 30, 40), 2, 0)
	if unknown[0] != 127 || unknown[1] != 255 {
		t.Fatalf("unknown length = %v, want [127 255]", unknown)
	}
	// More samples than announced land in the last bin instead of panicking.
	overrun := bucketPeaks(pcm(10, 10, 10, 10, 90), 2, 4)
	if overrun[1] != 255 {
		t.Fatalf("overrun = %v, want last bin loudest", overrun)
	}
}

func TestFFmpegWaveformGeneratorReadsARealFile(t *testing.T) {
	if _, err := exec.LookPath("ffmpeg"); err != nil {
		t.Skip("ffmpeg is not installed")
	}
	// One second: a loud first half and a quiet second half.
	samples := make([]int16, 44100)
	for index := range samples {
		amplitude := 0.9
		if index >= len(samples)/2 {
			amplitude = 0.1
		}
		samples[index] = int16(amplitude * math.MaxInt16 * math.Sin(float64(index)*2*math.Pi*440/44100))
	}
	path := filepath.Join(t.TempDir(), "tone.wav")
	if err := os.WriteFile(path, monoWAV(44100, samples), 0o600); err != nil {
		t.Fatal(err)
	}

	peaks, err := FFmpegWaveformGenerator(context.Background(), path, 1000)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	if len(peaks) != WaveformPeakCount {
		t.Fatalf("peak count = %d", len(peaks))
	}
	if peaks[10] < 200 || peaks[390] > 60 {
		t.Fatalf("loud half %d, quiet half %d", peaks[10], peaks[390])
	}
}

type waveformFixture struct {
	handlers *Handler
	store    *Repository
	service  *WaveformService
	trackID  string
	calls    atomic.Int32
}

func setupWaveform(t *testing.T, available bool, generate WaveformGenerator) *waveformFixture {
	t.Helper()
	db := testutil.OpenMigratedDB(t)
	store := NewRepository(db)
	filePath := filepath.Join(t.TempDir(), "track.flac")
	if err := os.WriteFile(filePath, []byte("audio"), 0o600); err != nil {
		t.Fatal(err)
	}
	_, trackID := testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{
		Title: "Song", Artist: "Artist", Album: "Album", TrackNo: 1, FilePath: filePath, DurationMs: 4000,
	})
	fixture := &waveformFixture{store: store, trackID: trackID}
	counted := func(ctx context.Context, path string, durationMs int) ([]byte, error) {
		fixture.calls.Add(1)
		return generate(ctx, path, durationMs)
	}
	fixture.service = NewWaveformService(store, counted, available)
	fixture.service.pendingAfter = 50 * time.Millisecond
	handlers := NewHandler(NewService(store))
	handlers.waveforms = fixture.service
	fixture.handlers = handlers
	return fixture
}

func (f *waveformFixture) get(t *testing.T, trackID string) *httptest.ResponseRecorder {
	t.Helper()
	request := httptest.NewRequest(http.MethodGet, "/api/v1/library/tracks/"+trackID+"/waveform", nil)
	routeContext := chi.NewRouteContext()
	routeContext.URLParams.Add("trackId", trackID)
	request = request.WithContext(context.WithValue(request.Context(), chi.RouteCtxKey, routeContext))
	recorder := httptest.NewRecorder()
	f.handlers.GetTrackWaveform(recorder, request)
	return recorder
}

func fakePeaks(ctx context.Context, path string, durationMs int) ([]byte, error) {
	return []byte{0, 128, 255}, nil
}

func TestWaveformHandlerGeneratesOnceThenServesTheCache(t *testing.T) {
	fixture := setupWaveform(t, true, fakePeaks)

	first := fixture.get(t, fixture.trackID)
	if first.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", first.Code, first.Body.String())
	}
	var body TrackWaveform
	if err := json.NewDecoder(first.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if body.TrackID != fixture.trackID || body.PeakCount != 3 || body.Peaks[2] != 255 {
		t.Fatalf("body = %+v", body)
	}
	if first.Header().Get("Cache-Control") != "private, no-cache" {
		t.Fatal("waveform responses must revalidate to detect Track Replacement")
	}

	second := fixture.get(t, fixture.trackID)
	if second.Code != http.StatusOK {
		t.Fatalf("second status = %d", second.Code)
	}
	if fixture.calls.Load() != 1 {
		t.Fatalf("generator ran %d times, want 1", fixture.calls.Load())
	}
}

func TestWaveformHandlerRegeneratesWhenTheFileChanges(t *testing.T) {
	fixture := setupWaveform(t, true, fakePeaks)
	fixture.get(t, fixture.trackID)
	source, err := fixture.store.GetTrackWaveformSource(context.Background(), fixture.trackID)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(source.FilePath, []byte("replaced audio"), 0o600); err != nil {
		t.Fatal(err)
	}

	if fixture.get(t, fixture.trackID).Code != http.StatusOK {
		t.Fatal("expected a fresh waveform")
	}
	if fixture.calls.Load() != 2 {
		t.Fatalf("generator ran %d times, want 2 after the file changed", fixture.calls.Load())
	}
}

func TestWaveformHandlerAnswersPendingWhileGenerationRuns(t *testing.T) {
	release := make(chan struct{})
	fixture := setupWaveform(t, true, func(ctx context.Context, path string, durationMs int) ([]byte, error) {
		<-release
		return fakePeaks(ctx, path, durationMs)
	})

	pending := fixture.get(t, fixture.trackID)
	if pending.Code != http.StatusAccepted {
		t.Fatalf("status = %d, want 202", pending.Code)
	}
	if pending.Header().Get("Retry-After") == "" {
		t.Fatal("pending response should carry Retry-After")
	}
	var body TrackWaveformPending
	if err := json.NewDecoder(pending.Body).Decode(&body); err != nil || body.Status != "pending" {
		t.Fatalf("body = %+v, err = %v", body, err)
	}

	close(release)
	deadline := time.Now().Add(2 * time.Second)
	for fixture.get(t, fixture.trackID).Code != http.StatusOK {
		if time.Now().After(deadline) {
			t.Fatal("waveform never finished")
		}
		time.Sleep(10 * time.Millisecond)
	}
	if fixture.calls.Load() != 1 {
		t.Fatalf("generator ran %d times, want 1 shared run", fixture.calls.Load())
	}
}

func TestWaveformHandlerReportsMissingTracksFilesAndDependencies(t *testing.T) {
	fixture := setupWaveform(t, true, fakePeaks)
	if code := fixture.get(t, "missing-track").Code; code != http.StatusNotFound {
		t.Fatalf("unknown track status = %d", code)
	}
	source, _ := fixture.store.GetTrackWaveformSource(context.Background(), fixture.trackID)
	if err := os.Remove(source.FilePath); err != nil {
		t.Fatal(err)
	}
	if code := fixture.get(t, fixture.trackID).Code; code != http.StatusNotFound {
		t.Fatalf("missing file status = %d", code)
	}

	unavailable := setupWaveform(t, false, fakePeaks)
	response := unavailable.get(t, unavailable.trackID)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503", response.Code)
	}
	if unavailable.calls.Load() != 0 {
		t.Fatal("generator must not run without ffmpeg")
	}

	failing := setupWaveform(t, true, func(context.Context, string, int) ([]byte, error) {
		return nil, errors.New("decoder exploded")
	})
	if code := failing.get(t, failing.trackID).Code; code != http.StatusInternalServerError {
		t.Fatalf("failed generation status = %d", code)
	}
}
