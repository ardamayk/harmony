package playback

import (
	"context"
	"errors"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

// Service owns queue rules: the item limit, track existence, item sources,
// and resolving stored track IDs into library tracks.
type Service struct {
	repo   *Repository
	tracks library.TrackReader
}

func NewService(repo *Repository, tracks library.TrackReader) *Service {
	return &Service{repo: repo, tracks: tracks}
}

// GetQueue returns the queue, skipping items whose track no longer exists.
func (s *Service) GetQueue(ctx context.Context, userID string) (Queue, error) {
	snapshot, err := s.repo.Read(ctx, userID)
	if err != nil {
		return Queue{}, err
	}
	items := make([]QueueItem, 0, len(snapshot.Items))
	for _, row := range snapshot.Items {
		source, err := parseQueueItemSource([]byte(row.Source))
		if err != nil {
			return Queue{}, fmt.Errorf("decode queue item %q source: %w", row.ID, err)
		}
		track, err := s.tracks.GetTrack(ctx, row.TrackID)
		if errors.Is(err, library.ErrNotFound) {
			continue
		}
		if err != nil {
			return Queue{}, fmt.Errorf("resolve queue track %q: %w", row.TrackID, err)
		}
		items = append(items, QueueItem{ID: row.ID, TrackID: row.TrackID, Position: row.Position, Track: track, Source: source})
	}
	return Queue{Items: items, Revision: snapshot.Revision, EventSequence: snapshot.EventSequence}, nil
}

func (s *Service) ReplaceQueue(ctx context.Context, userID string, trackIDs []string, expectedRevision string, sources ...QueueItemSource) (Queue, error) {
	if len(trackIDs) > MAX_QUEUE_ITEMS {
		return Queue{}, ErrQueueLimitExceeded
	}
	sourceJSON, err := encodeQueueItemSource(sources)
	if err != nil {
		return Queue{}, err
	}
	for _, trackID := range trackIDs {
		if err := s.requireTrack(ctx, trackID); err != nil {
			return Queue{}, err
		}
	}
	if err := s.repo.Replace(ctx, userID, expectedRevision, trackIDs, sourceJSON); err != nil {
		return Queue{}, err
	}
	return s.GetQueue(ctx, userID)
}

func (s *Service) AppendItem(ctx context.Context, userID, trackID, expectedRevision string, sources ...QueueItemSource) (Queue, error) {
	sourceJSON, err := encodeQueueItemSource(sources)
	if err != nil {
		return Queue{}, err
	}
	if err := s.requireTrack(ctx, trackID); err != nil {
		return Queue{}, err
	}
	if err := s.repo.Append(ctx, userID, expectedRevision, trackID, sourceJSON, MAX_QUEUE_ITEMS); err != nil {
		return Queue{}, err
	}
	return s.GetQueue(ctx, userID)
}

func (s *Service) RemoveItem(ctx context.Context, userID, itemID, expectedRevision string) (Queue, error) {
	if err := s.repo.Remove(ctx, userID, expectedRevision, itemID); err != nil {
		return Queue{}, err
	}
	return s.GetQueue(ctx, userID)
}

// ReorderItems accepts only a permutation of the current queue items.
func (s *Service) ReorderItems(ctx context.Context, userID string, itemIDs []string, expectedRevision string) (Queue, error) {
	err := s.repo.Reorder(ctx, userID, expectedRevision, itemIDs, func(current []string) bool {
		return sameQueueItems(current, itemIDs)
	})
	if err != nil {
		return Queue{}, err
	}
	return s.GetQueue(ctx, userID)
}

func (s *Service) requireTrack(ctx context.Context, trackID string) error {
	if _, err := s.tracks.GetTrack(ctx, trackID); err != nil {
		return library.ErrNotFound
	}
	return nil
}

func sameQueueItems(currentIDs, requestedIDs []string) bool {
	if len(currentIDs) != len(requestedIDs) {
		return false
	}
	seen := make(map[string]struct{}, len(currentIDs))
	for _, itemID := range currentIDs {
		seen[itemID] = struct{}{}
	}
	for _, itemID := range requestedIDs {
		if _, ok := seen[itemID]; !ok {
			return false
		}
		delete(seen, itemID)
	}
	return len(seen) == 0
}
