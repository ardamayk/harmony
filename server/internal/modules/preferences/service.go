package preferences

import "context"

// Service applies sparse preference patches on top of the stored values.
type Service struct {
	repo *Repository
}

func NewService(repo *Repository) *Service {
	return &Service{repo: repo}
}

func (s *Service) Get(ctx context.Context, userID string) (UserPreferences, error) {
	return s.repo.Get(ctx, userID)
}

func (s *Service) Patch(ctx context.Context, userID string, patch UserPreferencesPatch) (UserPreferences, error) {
	return s.repo.Update(ctx, userID, func(current *UserPreferences) {
		applyPatch(current, patch)
	})
}

func applyPatch(current *UserPreferences, patch UserPreferencesPatch) {
	if patch.Theme.Mode != "" {
		current.Theme.Mode = patch.Theme.Mode
	}
	if patch.Theme.Preset != "" {
		current.Theme.Preset = patch.Theme.Preset
	}
	if patch.Layout.SidebarPosition != "" {
		current.Layout.SidebarPosition = patch.Layout.SidebarPosition
	}
	for _, side := range []string{"left", "right"} {
		if panels := patch.Layout.Panels[side]; panels != nil {
			current.Layout.Panels[side] = panels
		}
		if collapsed, ok := patch.Layout.Collapsed[side]; ok {
			current.Layout.Collapsed[side] = collapsed
		}
	}
	if len(patch.Layout.Sizes) == 3 {
		current.Layout.Sizes = patch.Layout.Sizes
	}
	current.Layout = normalizeLayout(current.Layout)
	applyPlaybackPatch(&current.Playback, patch.Playback)
}
