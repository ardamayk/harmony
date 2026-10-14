package preferences

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/preferences/db"
)

// Repository stores one preferences row per user; theme, layout, and playback
// are persisted as JSON columns.
type Repository struct {
	sqlDB *sql.DB
	q     *db.Queries
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{sqlDB: sqlDB, q: db.New(sqlDB)}
}

func (r *Repository) Get(ctx context.Context, userID string) (UserPreferences, error) {
	return get(ctx, r.q, userID)
}

// Update reads the current preferences, applies mutate, and writes the result
// in one transaction. The database begins transactions IMMEDIATE, so the lock
// is taken before the read and concurrent sparse patches merge correctly.
func (r *Repository) Update(ctx context.Context, userID string, mutate func(*UserPreferences)) (UserPreferences, error) {
	tx, err := r.sqlDB.BeginTx(ctx, nil)
	if err != nil {
		return UserPreferences{}, fmt.Errorf("begin preferences patch: %w", err)
	}
	defer func() { _ = tx.Rollback() }()
	q := r.q.WithTx(tx)

	current, err := get(ctx, q, userID)
	if err != nil {
		return UserPreferences{}, err
	}
	mutate(&current)

	layoutJSON, err := json.Marshal(current.Layout)
	if err != nil {
		return UserPreferences{}, fmt.Errorf("encode layout: %w", err)
	}
	playbackJSON, err := json.Marshal(current.Playback)
	if err != nil {
		return UserPreferences{}, fmt.Errorf("encode playback: %w", err)
	}
	if err := q.UpdatePreferences(ctx, db.UpdatePreferencesParams{
		Theme:        themeToColumn(current.Theme),
		LayoutJson:   string(layoutJSON),
		PlaybackJson: string(playbackJSON),
		UserID:       userID,
	}); err != nil {
		return UserPreferences{}, fmt.Errorf("update preferences: %w", err)
	}
	if err := tx.Commit(); err != nil {
		return UserPreferences{}, fmt.Errorf("commit preferences patch: %w", err)
	}
	return current, nil
}

func get(ctx context.Context, q *db.Queries, userID string) (UserPreferences, error) {
	row, err := q.GetPreferences(ctx, userID)
	if err != nil {
		return UserPreferences{}, fmt.Errorf("get preferences: %w", err)
	}
	var layout LayoutPreferences
	if err := json.Unmarshal([]byte(row.LayoutJson), &layout); err != nil {
		return UserPreferences{}, fmt.Errorf("decode layout: %w", err)
	}
	return UserPreferences{
		Theme:    parseThemeColumn(row.Theme),
		Layout:   normalizeLayout(layout),
		Playback: parsePlaybackColumn(row.PlaybackJson),
	}, nil
}

func parseThemeColumn(raw string) ThemePreferences {
	var theme ThemePreferences
	if err := json.Unmarshal([]byte(raw), &theme); err == nil && theme.Mode != "" {
		if theme.Preset == "" {
			theme.Preset = "earthly"
		}
		return theme
	}
	switch raw {
	case "light", "dark", "system":
		return ThemePreferences{Mode: raw, Preset: "earthly"}
	default:
		return defaultTheme()
	}
}

func themeToColumn(theme ThemePreferences) string {
	if theme.Mode == "" {
		theme = defaultTheme()
	}
	if theme.Preset == "" {
		theme.Preset = "earthly"
	}
	b, _ := json.Marshal(theme)
	return string(b)
}
