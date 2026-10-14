package playback

import (
	"bufio"
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	api "github.com/ardam/navidrome-replacement/server/internal/api/gen"
	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

func setupPlaybackHandlers(t *testing.T) (*Handler, *Service, *library.Store, *sql.DB) {
	t.Helper()
	db := testutil.OpenMigratedDB(t)
	libStore := library.NewStore(db)
	libSvc := library.NewService(libStore)
	pbStore := newTestService(db, libStore)
	return NewHandler(pbStore, libSvc, NewQueueEventBroker()), pbStore, libStore, db
}

func seedPlaybackTrack(t *testing.T, db *sql.DB) string {
	t.Helper()
	_, trackID := testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{Title: "Song", Artist: "Artist", Album: "Album", TrackNo: 1})
	return trackID
}

func TestHandlersGetQueueEmpty(t *testing.T) {
	h, _, _, _ := setupPlaybackHandlers(t)
	rec := httptest.NewRecorder()
	h.GetQueue(rec, httptest.NewRequest(http.MethodGet, "/", nil))
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d", rec.Code)
	}
	var body Queue
	if err := json.NewDecoder(rec.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if len(body.Items) != 0 {
		t.Fatalf("items = %d, want 0", len(body.Items))
	}
	if body.Revision != "0" {
		t.Fatalf("revision = %q, want 0", body.Revision)
	}
}

func TestHandlersStreamQueueEventsStartsWithLatestQueueInvalidation(t *testing.T) {
	handlers, _, _, _ := setupPlaybackHandlers(t)
	server := httptest.NewServer(http.HandlerFunc(handlers.StreamQueueEvents))
	t.Cleanup(server.Close)

	response, err := server.Client().Get(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()

	if contentType := response.Header.Get("Content-Type"); contentType != "text/event-stream" {
		t.Fatalf("Content-Type = %q, want text/event-stream", contentType)
	}
	event := readQueueEvent(t, bufio.NewScanner(response.Body))
	if event.ID != "0" || event.Name != "queue-invalidated" {
		t.Fatalf("event = %+v, want initial Queue revision 0 invalidation", event)
	}
	if event.Data.Revision != "0" || event.Data.Sequence != "0" || len(event.Data.Invalidates) != 1 || event.Data.Invalidates[0] != api.QueueEventInvalidatesQueue {
		t.Fatalf("event data = %+v", event.Data)
	}
}

func TestHandlersStreamQueueEventsNotifiesSimultaneousClients(t *testing.T) {
	handlers, _, _, db := setupPlaybackHandlers(t)
	trackID := seedPlaybackTrack(t, db)
	router := chi.NewRouter()
	router.Get("/events", handlers.StreamQueueEvents)
	router.Post("/items", handlers.AppendItem)
	server := httptest.NewServer(router)
	t.Cleanup(server.Close)

	firstResponse, firstEvents := openQueueEventStream(t, server, "")
	defer func() { _ = firstResponse.Body.Close() }()
	secondResponse, secondEvents := openQueueEventStream(t, server, "")
	defer func() { _ = secondResponse.Body.Close() }()
	_ = readQueueEvent(t, firstEvents)
	_ = readQueueEvent(t, secondEvents)

	body, _ := json.Marshal(map[string]string{"trackId": trackID, "revision": "0"})
	response, err := server.Client().Post(server.URL+"/items", "application/json", bytes.NewReader(body))
	if err != nil {
		t.Fatal(err)
	}
	_ = response.Body.Close()
	if response.StatusCode != http.StatusOK {
		t.Fatalf("mutation status = %d, want 200", response.StatusCode)
	}

	for clientIndex, scanner := range []*bufio.Scanner{firstEvents, secondEvents} {
		event := readQueueEvent(t, scanner)
		if event.ID != "1" || event.Data.Revision != "1" {
			t.Fatalf("client %d event = %+v, want revision 1", clientIndex+1, event)
		}
	}
}

func TestHandlersStreamQueueEventsReconnectsWithLatestRevision(t *testing.T) {
	handlers, store, _, db := setupPlaybackHandlers(t)
	trackID := seedPlaybackTrack(t, db)
	router := chi.NewRouter()
	router.Get("/events", handlers.StreamQueueEvents)
	server := httptest.NewServer(router)
	t.Cleanup(server.Close)

	initialResponse, initialEvents := openQueueEventStream(t, server, "")
	initial := readQueueEvent(t, initialEvents)
	_ = initialResponse.Body.Close()
	if initial.Data.Revision != "0" {
		t.Fatalf("initial revision = %q, want 0", initial.Data.Revision)
	}
	if _, err := store.AppendItem(context.Background(), auth.DefaultUserID, trackID, "0"); err != nil {
		t.Fatal(err)
	}

	reconnectedResponse, reconnectedEvents := openQueueEventStream(t, server, initial.ID)
	defer func() { _ = reconnectedResponse.Body.Close() }()
	reconnected := readQueueEvent(t, reconnectedEvents)
	if reconnected.ID != "1" || reconnected.Data.Revision != "1" {
		t.Fatalf("reconnected event = %+v, want latest revision 1", reconnected)
	}
}

func TestHandlersStreamQueueEventsUsesPersistedSequenceIndependentOfRevision(t *testing.T) {
	handlers, store, _, db := setupPlaybackHandlers(t)
	trackID := seedPlaybackTrack(t, db)
	if _, err := store.AppendItem(context.Background(), auth.DefaultUserID, trackID, "0"); err != nil {
		t.Fatal(err)
	}
	if _, err := db.Exec(`UPDATE playback_queue_state SET event_sequence = 41 WHERE user_id = ?`, auth.DefaultUserID); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(handlers.StreamQueueEvents))
	t.Cleanup(server.Close)

	response, events := openQueueEventStream(t, server, "")
	defer func() { _ = response.Body.Close() }()
	event := readQueueEvent(t, events)
	if event.ID != "41" || event.Data.Sequence != "41" || event.Data.Revision != "1" {
		t.Fatalf("event = %+v, want persisted sequence 41 and revision 1", event)
	}
}

func openQueueEventStream(t *testing.T, server *httptest.Server, lastEventID string) (*http.Response, *bufio.Scanner) {
	t.Helper()
	request, err := http.NewRequest(http.MethodGet, server.URL+"/events", nil)
	if err != nil {
		t.Fatal(err)
	}
	if lastEventID != "" {
		request.Header.Set("Last-Event-ID", lastEventID)
	}
	response, err := server.Client().Do(request)
	if err != nil {
		t.Fatal(err)
	}
	return response, bufio.NewScanner(response.Body)
}

type queueServerEvent struct {
	ID   string
	Name string
	Data api.QueueEvent
}

func readQueueEvent(t *testing.T, scanner *bufio.Scanner) queueServerEvent {
	t.Helper()
	var event queueServerEvent
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" {
			return event
		}
		if value, found := strings.CutPrefix(line, "id: "); found {
			event.ID = value
		}
		if value, found := strings.CutPrefix(line, "event: "); found {
			event.Name = value
		}
		if value, found := strings.CutPrefix(line, "data: "); found {
			if err := json.Unmarshal([]byte(value), &event.Data); err != nil {
				t.Fatal(err)
			}
		}
	}
	t.Fatalf("Queue event stream ended: %v", scanner.Err())
	return queueServerEvent{}
}

func TestHandlersReplaceQueue(t *testing.T) {
	h, _, _, db := setupPlaybackHandlers(t)
	trackID := seedPlaybackTrack(t, db)

	body, _ := json.Marshal(map[string]any{"trackIds": []string{trackID}, "revision": "0"})
	req := httptest.NewRequest(http.MethodPut, "/", bytes.NewReader(body))
	rec := httptest.NewRecorder()
	h.ReplaceQueue(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
	var queue Queue
	if err := json.NewDecoder(rec.Body).Decode(&queue); err != nil {
		t.Fatal(err)
	}
	if len(queue.Items) != 1 || queue.Items[0].TrackID != trackID {
		t.Fatalf("queue = %+v", queue.Items)
	}
}

type staleHandlerMutation func(*Handler, Queue, string) *httptest.ResponseRecorder

func staleReplace(handlers *Handler, _ Queue, trackID string) *httptest.ResponseRecorder {
	body, _ := json.Marshal(map[string]any{"trackIds": []string{trackID}, "revision": "0"})
	recorder := httptest.NewRecorder()
	handlers.ReplaceQueue(recorder, httptest.NewRequest(http.MethodPut, "/", bytes.NewReader(body)))
	return recorder
}

func staleAppend(handlers *Handler, _ Queue, trackID string) *httptest.ResponseRecorder {
	body, _ := json.Marshal(map[string]string{"trackId": trackID, "revision": "0"})
	recorder := httptest.NewRecorder()
	handlers.AppendItem(recorder, httptest.NewRequest(http.MethodPost, "/", bytes.NewReader(body)))
	return recorder
}

func staleReorder(handlers *Handler, queue Queue, _ string) *httptest.ResponseRecorder {
	body, _ := json.Marshal(map[string]any{"itemIds": []string{queue.Items[0].ID}, "revision": "0"})
	recorder := httptest.NewRecorder()
	handlers.ReorderQueue(recorder, httptest.NewRequest(http.MethodPatch, "/", bytes.NewReader(body)))
	return recorder
}

func staleRemove(handlers *Handler, queue Queue, _ string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodDelete, "/", nil)
	request.Header.Set("If-Match", "0")
	routeContext := chi.NewRouteContext()
	routeContext.URLParams.Add("itemId", queue.Items[0].ID)
	request = request.WithContext(context.WithValue(request.Context(), chi.RouteCtxKey, routeContext))
	recorder := httptest.NewRecorder()
	handlers.RemoveItem(recorder, request)
	return recorder
}

func assertQueueConflict(t *testing.T, recorder *httptest.ResponseRecorder) {
	t.Helper()
	if recorder.Code != http.StatusConflict {
		t.Fatalf("status = %d, body %s", recorder.Code, recorder.Body.String())
	}
	var conflict struct {
		Code  string `json:"code"`
		Queue Queue  `json:"queue"`
	}
	if err := json.NewDecoder(recorder.Body).Decode(&conflict); err != nil {
		t.Fatal(err)
	}
	if conflict.Code != "queue_revision_conflict" || conflict.Queue.Revision != "1" || len(conflict.Queue.Items) != 1 {
		t.Fatalf("conflict = %+v", conflict)
	}
}

func TestHandlersReturnCurrentQueueForEveryStaleMutation(t *testing.T) {
	tests := map[string]staleHandlerMutation{
		"replace": staleReplace,
		"append":  staleAppend,
		"reorder": staleReorder,
		"remove":  staleRemove,
	}

	for name, mutate := range tests {
		t.Run(name, func(t *testing.T) {
			handlers, store, _, db := setupPlaybackHandlers(t)
			trackID := seedPlaybackTrack(t, db)
			queue, err := store.AppendItem(context.Background(), "00000000-0000-0000-0000-000000000001", trackID, "0")
			if err != nil {
				t.Fatal(err)
			}

			assertQueueConflict(t, mutate(handlers, queue, trackID))
		})
	}
}

func TestHandlersReorderQueue(t *testing.T) {
	h, store, _, db := setupPlaybackHandlers(t)
	trackID := seedPlaybackTrack(t, db)
	queue, err := store.AppendItem(context.Background(), "00000000-0000-0000-0000-000000000001", trackID, "0")
	if err != nil {
		t.Fatal(err)
	}
	body, _ := json.Marshal(map[string]any{"itemIds": []string{queue.Items[0].ID}, "revision": "1"})
	rec := httptest.NewRecorder()
	h.ReorderQueue(rec, httptest.NewRequest(http.MethodPatch, "/", bytes.NewReader(body)))

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
	var reordered Queue
	if err := json.NewDecoder(rec.Body).Decode(&reordered); err != nil {
		t.Fatal(err)
	}
	if reordered.Revision != "2" {
		t.Fatalf("revision = %q, want 2", reordered.Revision)
	}
}

func TestHandlersAppendItemRequiresTrackID(t *testing.T) {
	h, _, _, _ := setupPlaybackHandlers(t)
	req := httptest.NewRequest(http.MethodPost, "/", bytes.NewReader([]byte(`{}`)))
	rec := httptest.NewRecorder()
	h.AppendItem(rec, req)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", rec.Code)
	}
}

func TestHandlersStreamTrackNotFound(t *testing.T) {
	h, _, _, _ := setupPlaybackHandlers(t)
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	rctx := chi.NewRouteContext()
	rctx.URLParams.Add("trackId", "missing")
	req = req.WithContext(context.WithValue(req.Context(), chi.RouteCtxKey, rctx))

	h.StreamTrack(rec, req)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", rec.Code)
	}
}

func TestTrackHeadRouteChecksExistingAndMissingAudio(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	filePath := filepath.Join(t.TempDir(), "audio.flac")
	if err := os.WriteFile(filePath, []byte("audio"), 0o600); err != nil {
		t.Fatal(err)
	}
	_, trackID := testutil.SeedManagedTrack(t, database, testutil.ManagedTrackSpec{FilePath: filePath})
	tracks := library.NewService(library.NewStore(database))
	router := chi.NewRouter()
	NewModule(database, tracks).RegisterRoutes(router)
	server := httptest.NewServer(router)
	defer server.Close()
	checkTrackHead(t, server.URL+"/api/v1/tracks/"+trackID+"/stream", http.StatusOK)
	if err := os.Remove(filePath); err != nil {
		t.Fatal(err)
	}
	checkTrackHead(t, server.URL+"/api/v1/tracks/"+trackID+"/stream", http.StatusNotFound)
	checkTrackHead(t, server.URL+"/api/v1/tracks/unknown/stream", http.StatusNotFound)
}

func checkTrackHead(t *testing.T, url string, status int) {
	t.Helper()
	response, err := http.Head(url)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()
	body, readErr := io.ReadAll(response.Body)
	if readErr != nil {
		t.Fatal(readErr)
	}
	if response.StatusCode != status || len(body) != 0 {
		t.Fatalf("HEAD status = %d, body bytes = %d; want %d with no body", response.StatusCode, len(body), status)
	}
}
