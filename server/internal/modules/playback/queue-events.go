package playback

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"sync"
	"time"

	api "github.com/ardam/navidrome-replacement/server/internal/api/gen"
	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
	"github.com/ardam/navidrome-replacement/server/internal/auth"
)

const (
	queueEventName      = "queue-invalidated"
	queueEventKeepAlive = 15 * time.Second
)

type QueueInvalidation struct {
	Revision string
	Sequence string
}

type QueueEventBroker struct {
	mu          sync.Mutex
	subscribers map[string]map[chan QueueInvalidation]struct{}
}

func NewQueueEventBroker() *QueueEventBroker {
	return &QueueEventBroker{subscribers: make(map[string]map[chan QueueInvalidation]struct{})}
}

func (b *QueueEventBroker) Subscribe(userID string) (<-chan QueueInvalidation, func()) {
	events := make(chan QueueInvalidation, 1)
	b.mu.Lock()
	if b.subscribers[userID] == nil {
		b.subscribers[userID] = make(map[chan QueueInvalidation]struct{})
	}
	b.subscribers[userID][events] = struct{}{}
	b.mu.Unlock()
	return events, func() { b.unsubscribe(userID, events) }
}

func (b *QueueEventBroker) Publish(userID, revision, sequence string) {
	event := QueueInvalidation{Revision: revision, Sequence: sequence}
	b.mu.Lock()
	defer b.mu.Unlock()
	for subscriber := range b.subscribers[userID] {
		publishLatestQueueEvent(subscriber, event)
	}
}

func (b *QueueEventBroker) unsubscribe(userID string, events chan QueueInvalidation) {
	b.mu.Lock()
	defer b.mu.Unlock()
	delete(b.subscribers[userID], events)
	if len(b.subscribers[userID]) == 0 {
		delete(b.subscribers, userID)
	}
}

func publishLatestQueueEvent(events chan QueueInvalidation, event QueueInvalidation) {
	select {
	case events <- event:
		return
	default:
	}
	select {
	case <-events:
	default:
	}
	select {
	case events <- event:
	default:
	}
}

func (h *Handler) StreamQueueEvents(w http.ResponseWriter, r *http.Request) {
	userID, err := auth.CurrentUserID(r)
	if err != nil {
		respond.Error(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}
	flusher, ok := w.(http.Flusher)
	if !ok {
		respond.Error(w, http.StatusInternalServerError, "internal_error", "streaming is unsupported")
		return
	}
	events, unsubscribe := h.queueEvents.Subscribe(userID)
	defer unsubscribe()
	queue, err := h.service.GetQueue(r.Context(), userID)
	if err != nil {
		respond.Error(w, http.StatusInternalServerError, "internal_error", err.Error())
		return
	}
	setQueueEventHeaders(w.Header())
	if !writeQueueEvent(w, flusher, newQueueInvalidation(queue.Revision, queue.EventSequence)) {
		return
	}
	h.streamQueueEvents(w, r, flusher, events)
}

func (h *Handler) streamQueueEvents(w http.ResponseWriter, r *http.Request, flusher http.Flusher, events <-chan QueueInvalidation) {
	keepAlive := time.NewTicker(queueEventKeepAlive)
	defer keepAlive.Stop()
	for {
		select {
		case <-r.Context().Done():
			return
		case event := <-events:
			if !writeQueueEvent(w, flusher, event) {
				return
			}
		case <-keepAlive.C:
			if _, err := fmt.Fprint(w, ": keep-alive\n\n"); err != nil {
				return
			}
			flusher.Flush()
		}
	}
}

func newQueueInvalidation(revision, sequence string) QueueInvalidation {
	return QueueInvalidation{Revision: revision, Sequence: sequence}
}

func setQueueEventHeaders(header http.Header) {
	header.Set("Content-Type", "text/event-stream")
	header.Set("Cache-Control", "no-cache")
	header.Set("Connection", "keep-alive")
	header.Set("X-Accel-Buffering", "no")
}

func writeQueueEvent(w http.ResponseWriter, flusher http.Flusher, invalidation QueueInvalidation) bool {
	event := api.QueueEvent{
		Invalidates: []api.QueueEventInvalidates{api.QueueEventInvalidatesQueue},
		Revision:    invalidation.Revision,
		Sequence:    invalidation.Sequence,
	}
	data, err := json.Marshal(event)
	if err != nil {
		slog.Error("encode Queue event", "revision", invalidation.Revision, "error", err)
		return false
	}
	if _, err := fmt.Fprintf(w, "id: %s\nevent: %s\ndata: %s\n\n", invalidation.Sequence, queueEventName, data); err != nil {
		return false
	}
	flusher.Flush()
	return true
}
