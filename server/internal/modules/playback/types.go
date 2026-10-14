package playback

import (
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

const MAX_QUEUE_ITEMS = 1000

var ErrQueueLimitExceeded = fmt.Errorf("queue exceeds maximum of %d items", MAX_QUEUE_ITEMS)

type QueueItem struct {
	ID       string          `json:"id"`
	TrackID  string          `json:"trackId"`
	Position int             `json:"position"`
	Track    library.Track   `json:"track"`
	Source   QueueItemSource `json:"source"`
}

type Queue struct {
	Items         []QueueItem `json:"items"`
	Revision      string      `json:"revision"`
	EventSequence string      `json:"-"`
}
