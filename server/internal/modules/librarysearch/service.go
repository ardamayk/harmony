package librarysearch

import (
	"context"
	"sync"
)

// Service ranks Library Search queries over a cached index that is reloaded
// whenever the search revision changes.
type Service struct {
	repo *Repository

	mu       sync.Mutex
	index    *Index
	revision int64
	userID   string
}

func NewService(repo *Repository) *Service {
	return &Service{repo: repo}
}

// Search evaluates ranking over the full eligible library. Storage failures
// propagate as errors rather than as an empty success.
func (s *Service) Search(ctx context.Context, userID string, q query, all bool) (rankedResults, error) {
	index, err := s.currentIndex(ctx, userID)
	if err != nil {
		return rankedResults{}, err
	}
	return rank(index, q, all), nil
}

func (s *Service) currentIndex(ctx context.Context, userID string) (*Index, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	index, revision, err := s.repo.LoadIndex(ctx, userID, func(revision int64) bool {
		return s.index != nil && s.revision == revision && s.userID == userID
	})
	if err != nil {
		return nil, err
	}
	if index == nil {
		return s.index, nil
	}
	// ponytail: one active user's immutable snapshot; use bounded per-user
	// snapshots if multi-user search becomes a supported workload.
	s.index, s.revision, s.userID = index, revision, userID
	return index, nil
}
