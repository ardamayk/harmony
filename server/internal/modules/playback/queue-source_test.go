package playback

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

func TestQueueSourcePersistsThroughReorderAndRemoval(t *testing.T) {
	sources := []QueueItemSource{
		{Kind: "album", AlbumID: "album-1", AlbumTitle: "Album", ArtistName: "Artist"},
		{Kind: "playlist", PlaylistID: "playlist-1", Name: "Playlist"},
		{Kind: "user"},
		{Kind: "suggestion", BasedOn: []string{"seed-1", "seed-2"}},
		{Kind: "suggestion", BasedOn: []string{}},
	}
	for _, source := range sources {
		t.Run(fmt.Sprintf("%s/%v", source.Kind, source.BasedOn), func(t *testing.T) {
			assertQueueSourcePersistence(t, source)
		})
	}
}

func assertQueueSourcePersistence(t *testing.T, source QueueItemSource) {
	t.Helper()
	ctx := context.Background()
	store := setupPlaybackStore(t, map[string]library.Track{"track-1": {ID: "track-1"}})
	queue, err := store.ReplaceQueue(ctx, "user-1", []string{"track-1", "track-1"}, "0", source)
	if err != nil {
		t.Fatal(err)
	}
	for _, item := range queue.Items {
		assertQueueSource(t, item.Source, source)
	}
	firstID := queue.Items[0].ID
	queue, err = store.AppendItem(ctx, "user-1", "track-1", queue.Revision)
	if err != nil {
		t.Fatal(err)
	}
	userItemID := queue.Items[2].ID
	queue, err = store.AppendItem(ctx, "user-1", "track-1", queue.Revision, source)
	if err != nil {
		t.Fatal(err)
	}
	appendedID := queue.Items[3].ID
	assertQueueSource(t, queue.Items[3].Source, source)
	queue, err = store.ReorderItems(ctx, "user-1", []string{appendedID, userItemID, firstID, queue.Items[1].ID}, queue.Revision)
	if err != nil {
		t.Fatal(err)
	}
	assertQueueSource(t, queue.Items[0].Source, source)
	assertQueueSource(t, queue.Items[1].Source, QueueItemSource{Kind: "user"})
	queue, err = store.RemoveItem(ctx, "user-1", userItemID, queue.Revision)
	if err != nil {
		t.Fatal(err)
	}
	// Reading back from SQLite returns the same source, including duplicate tracks.
	queue, err = store.GetQueue(ctx, "user-1")
	if err != nil {
		t.Fatal(err)
	}
	if queue.Revision != "5" || queue.EventSequence != "5" || len(queue.Items) != 3 {
		t.Fatalf("unexpected persisted queue: %+v", queue)
	}
	for position, item := range queue.Items {
		assertQueueSource(t, item.Source, source)
		if item.Position != position {
			t.Fatalf("position = %d, want %d", item.Position, position)
		}
	}
}

func assertQueueSource(t *testing.T, got, want QueueItemSource) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("source = %+v, want %+v", got, want)
	}
}

func TestHandlersRejectInvalidSourcesWithoutMutationOrEvent(t *testing.T) {
	invalidSources := []string{
		fmt.Sprintf(`{"kind":"playlist","playlistId":"p","name":%q}`, strings.Repeat("x", MAX_QUEUE_SOURCE_STRING_LENGTH+1)),
		`{"kind":"suggestion","basedOn":[` + strings.Repeat(`"seed",`, MAX_QUEUE_SOURCE_BASED_ON) + `"seed"]}`,
		fmt.Sprintf(`{"kind":"suggestion","basedOn":[%q]}`, strings.Repeat("x", MAX_QUEUE_SOURCE_STRING_LENGTH+1)),
		`{"kind":` + strings.Repeat(" ", MAX_QUEUE_SOURCE_BYTES) + `"user"}`,

		`null`, `[]`, `"user"`, `{}`, `{"kind":"other"}`,
		`{"kind":"album","albumId":"a","albumTitle":"Album"}`,
		`{"kind":"album","albumId":"a","albumTitle":"Album","artistName":" "}`,
		`{"kind":"playlist","playlistId":"p","name":null}`,
		`{"kind":"playlist","playlistId":12,"name":"Playlist"}`,
		`{"kind":"user","albumId":"a"}`,
		`{"kind":"user","extra":null}`,
		`{"kind":"suggestion"}`, `{"kind":"suggestion","basedOn":null}`,
		`{"kind":"suggestion","basedOn":"seed"}`,
		`{"kind":"suggestion","basedOn":[null]}`,
		`{"kind":"suggestion","basedOn":[" "]}`,
	}
	handlers, store, _, database := setupPlaybackHandlers(t)
	trackID := seedPlaybackTrack(t, database)
	for _, method := range []string{http.MethodPut, http.MethodPost} {
		for _, source := range invalidSources {
			t.Run(method+"/"+source, func(t *testing.T) {
				events, unsubscribe := handlers.queueEvents.Subscribe(auth.DefaultUserID)
				defer unsubscribe()
				response := requestQueueSource(handlers, method, trackID, "0", source)
				if response.Code != http.StatusBadRequest {
					t.Fatalf("status = %d, body %s", response.Code, response.Body)
				}
				queue, err := store.GetQueue(context.Background(), auth.DefaultUserID)
				if err != nil {
					t.Fatal(err)
				}
				if queue.Revision != "0" || queue.EventSequence != "0" || len(queue.Items) != 0 {
					t.Fatalf("invalid source mutated queue: %+v", queue)
				}
				select {
				case event := <-events:
					t.Fatalf("invalid source published event: %+v", event)
				default:
				}
			})
		}
	}
}

func requestQueueSource(handlers *Handler, method, trackID, revision, source string) *httptest.ResponseRecorder {
	body := fmt.Sprintf(`{"trackIds":[%q,%q],"trackId":%q,"revision":%q`, trackID, trackID, trackID, revision)
	if source != "" {
		body += `,"source":` + source
	}
	request := httptest.NewRequest(method, "/", strings.NewReader(body+"}"))
	response := httptest.NewRecorder()
	if method == http.MethodPut {
		handlers.ReplaceQueue(response, request)
	} else {
		handlers.AppendItem(response, request)
	}
	return response
}

func TestHandlersReturnSourceAndPreserveRevisionEvents(t *testing.T) {
	handlers, _, _, database := setupPlaybackHandlers(t)
	trackID := seedPlaybackTrack(t, database)
	events, unsubscribe := handlers.queueEvents.Subscribe(auth.DefaultUserID)
	defer unsubscribe()
	album := `{"kind":"album","albumId":"a","albumTitle":"Album","artistName":"Artist"}`
	response := requestQueueSource(handlers, http.MethodPut, trackID, "0", album)
	queue := decodeSourceQueue(t, response)
	for _, item := range queue.Items {
		if item.Source.Kind != "album" || item.Source.AlbumTitle != "Album" {
			t.Fatalf("replace lost album source: %+v", item)
		}
	}
	assertSourceEvent(t, events, "1")
	response = requestQueueSource(handlers, http.MethodPost, trackID, "1", `{"kind":"suggestion","basedOn":[]}`)
	queue = decodeSourceQueue(t, response)
	assertQueueSource(t, queue.Items[2].Source, QueueItemSource{Kind: "suggestion", BasedOn: []string{}})
	assertSourceEvent(t, events, "2")
	response = requestQueueSource(handlers, http.MethodPost, trackID, "2", "")
	queue = decodeSourceQueue(t, response)
	assertQueueSource(t, queue.Items[3].Source, QueueItemSource{Kind: "user"})
	assertSourceEvent(t, events, "3")
	response = requestQueueSource(handlers, http.MethodPut, trackID, "0", `{"kind":"user"}`)
	if response.Code != http.StatusConflict {
		t.Fatalf("status = %d, want conflict", response.Code)
	}
	var conflict struct {
		Queue Queue `json:"queue"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &conflict); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(conflict.Queue, queue) {
		t.Fatalf("conflict lost queue sources: %+v", conflict.Queue)
	}
	select {
	case event := <-events:
		t.Fatalf("stale source mutation published event: %+v", event)
	default:
	}
}

func decodeSourceQueue(t *testing.T, response *httptest.ResponseRecorder) Queue {
	t.Helper()
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body %s", response.Code, response.Body)
	}
	var queue Queue
	if err := json.Unmarshal(response.Body.Bytes(), &queue); err != nil {
		t.Fatal(err)
	}
	return queue
}

func assertSourceEvent(t *testing.T, events <-chan QueueInvalidation, revision string) {
	t.Helper()
	select {
	case event := <-events:
		if event.Revision != revision || event.Sequence != revision {
			t.Fatalf("event = %+v, want revision and sequence %s", event, revision)
		}
	default:
		t.Fatal("source mutation did not publish queue invalidation")
	}
}

func TestStoreRejectsInvalidSource(t *testing.T) {
	store := setupPlaybackStore(t, map[string]library.Track{"track-1": {ID: "track-1"}})
	source := QueueItemSource{Kind: "album", AlbumID: "album-1"}
	if _, err := store.ReplaceQueue(context.Background(), "user-1", []string{"track-1"}, "0", source); !errors.Is(err, ErrInvalidQueueSource) {
		t.Fatalf("replace error = %v, want ErrInvalidQueueSource", err)
	}
	if _, err := store.AppendItem(context.Background(), "user-1", "track-1", "0", source); !errors.Is(err, ErrInvalidQueueSource) {
		t.Fatalf("append error = %v, want ErrInvalidQueueSource", err)
	}
	queue, err := store.GetQueue(context.Background(), "user-1")
	if err != nil {
		t.Fatal(err)
	}
	if queue.Revision != "0" || queue.EventSequence != "0" || len(queue.Items) != 0 {
		t.Fatalf("invalid source mutated queue: %+v", queue)
	}
}
