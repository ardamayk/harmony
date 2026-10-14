package preferences

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func intPtr(v int) *int           { return &v }
func floatPtr(v float64) *float64 { return &v }
func boolPtr(v bool) *bool        { return &v }
func ctx() context.Context        { return context.Background() }
func newService(t *testing.T) *Service {
	t.Helper()
	return NewService(NewRepository(testutil.OpenMigratedDB(t)))
}

func TestGetReturnsDefaultPlaybackPreferencesForSeededRow(t *testing.T) {
	store := newService(t)
	prefs, err := store.Get(ctx(), auth.DefaultUserID)
	if err != nil {
		t.Fatal(err)
	}
	if prefs.Playback != defaultPlayback() {
		t.Fatalf("playback = %+v, want defaults", prefs.Playback)
	}
}

func TestPatchPlaybackKeepsUntouchedFieldsAndPersistsFalse(t *testing.T) {
	store := newService(t)
	updated, err := store.Patch(ctx(), auth.DefaultUserID, UserPreferencesPatch{
		Playback: &PlaybackPreferencesPatch{
			SeekStepSeconds: intPtr(10),
			PlaybackRate:    floatPtr(1.25),
			ShowWaveform:    boolPtr(false),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if updated.Playback.SeekStepSeconds != 10 || updated.Playback.PlaybackRate != 1.25 {
		t.Fatalf("numeric fields not applied: %+v", updated.Playback)
	}
	if updated.Playback.ShowWaveform {
		t.Fatal("showWaveform=false was not persisted")
	}
	if updated.Playback.SeekStepLargeSeconds != 30 || !updated.Playback.AccentFromCover {
		t.Fatalf("untouched fields changed: %+v", updated.Playback)
	}

	reread, err := store.Get(ctx(), auth.DefaultUserID)
	if err != nil {
		t.Fatal(err)
	}
	if reread.Playback != updated.Playback {
		t.Fatalf("reread = %+v, want %+v", reread.Playback, updated.Playback)
	}
}

func TestPatchWithoutPlaybackLeavesPlaybackAlone(t *testing.T) {
	store := newService(t)
	if _, err := store.Patch(ctx(), auth.DefaultUserID, UserPreferencesPatch{
		Playback: &PlaybackPreferencesPatch{HoverTimestamp: boolPtr(false)},
	}); err != nil {
		t.Fatal(err)
	}
	updated, err := store.Patch(ctx(), auth.DefaultUserID, UserPreferencesPatch{
		Theme: ThemePreferences{Mode: "dark"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if updated.Theme.Mode != "dark" || updated.Playback.HoverTimestamp {
		t.Fatalf("unexpected preferences after theme patch: %+v", updated)
	}
}

func TestConcurrentPlaybackPatchesPreserveIndependentFields(t *testing.T) {
	store := newService(t)
	const concurrentRounds = 20
	for round := range concurrentRounds {
		seekStep := 10 + round
		seekStepLarge := 40 + round
		fade := 100 + round
		autoSkip := 10 + round
		patches := []PlaybackPreferencesPatch{
			{SeekStepSeconds: &seekStep},
			{SeekStepLargeSeconds: &seekStepLarge},
			{TransitionFadeMs: &fade},
			{AutoSkipOnErrorSeconds: &autoSkip},
		}
		start := make(chan struct{})
		results := make(chan error, len(patches))
		for _, patch := range patches {
			go func() {
				<-start
				_, err := store.Patch(ctx(), auth.DefaultUserID, UserPreferencesPatch{Playback: &patch})
				results <- err
			}()
		}
		close(start)
		for range patches {
			if err := <-results; err != nil {
				t.Errorf("round %d: patch preferences: %v", round, err)
			}
		}
		current, err := store.Get(ctx(), auth.DefaultUserID)
		if err != nil {
			t.Fatal(err)
		}
		if current.Playback.SeekStepSeconds != seekStep || current.Playback.SeekStepLargeSeconds != seekStepLarge ||
			current.Playback.TransitionFadeMs != fade || current.Playback.AutoSkipOnErrorSeconds != autoSkip {
			t.Fatalf("round %d: concurrent patches lost updates: %+v", round, current.Playback)
		}
	}
}

func TestParsePlaybackColumnFillsMissingFieldsWithDefaults(t *testing.T) {
	playback := parsePlaybackColumn(`{"seekStepSeconds":15}`)
	if playback.SeekStepSeconds != 15 || playback.AutoSkipOnErrorSeconds != 5 || !playback.ShowUpNext {
		t.Fatalf("playback = %+v", playback)
	}
	if parsePlaybackColumn("not json") != defaultPlayback() {
		t.Fatal("invalid JSON should fall back to defaults")
	}
}

func TestValidatePlaybackPatchRejectsOutOfRangeValues(t *testing.T) {
	cases := map[string]PlaybackPreferencesPatch{
		"seek too small":  {SeekStepSeconds: intPtr(0)},
		"seek too large":  {SeekStepLargeSeconds: intPtr(121)},
		"rate too slow":   {PlaybackRate: floatPtr(0.25)},
		"rate too fast":   {PlaybackRate: floatPtr(3)},
		"fade negative":   {TransitionFadeMs: intPtr(-1)},
		"fade too long":   {TransitionFadeMs: intPtr(5000)},
		"auto skip large": {AutoSkipOnErrorSeconds: intPtr(61)},
	}
	for name, patch := range cases {
		if err := ValidatePlaybackPatch(&patch); err == nil {
			t.Errorf("%s: expected an error", name)
		}
	}
	if err := ValidatePlaybackPatch(&PlaybackPreferencesPatch{PlaybackRate: floatPtr(2), TransitionFadeMs: intPtr(0)}); err != nil {
		t.Fatalf("boundary values rejected: %v", err)
	}
}

func TestHandlePatchRejectsInvalidPlaybackAndAppliesValid(t *testing.T) {
	handler := NewHandler(newService(t))

	rec := httptest.NewRecorder()
	handler.Patch(rec, httptest.NewRequest(http.MethodPatch, "/api/v1/preferences",
		strings.NewReader(`{"playback":{"playbackRate":9}}`)))
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", rec.Code)
	}

	rec = httptest.NewRecorder()
	handler.Patch(rec, httptest.NewRequest(http.MethodPatch, "/api/v1/preferences",
		strings.NewReader(`{"playback":{"autoSkipOnErrorSeconds":0,"showUpNext":false}}`)))
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
	var body UserPreferences
	if err := json.NewDecoder(rec.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if body.Playback.AutoSkipOnErrorSeconds != 0 || body.Playback.ShowUpNext {
		t.Fatalf("playback = %+v", body.Playback)
	}
}
