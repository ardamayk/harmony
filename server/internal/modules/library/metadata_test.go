package library

import (
	"testing"
)

func TestReadReplayGainStringMetadata(t *testing.T) {
	metadata := readReplayGainStringMetadata(map[string]string{
		"replaygain_track_gain": "-7.25 dB",
		"replaygain_track_peak": "0.987654",
		"replaygain_album_gain": "-6.50 dB",
		"replaygain_album_peak": "1.012345",
	})

	assertReplayGainMetadata(t, metadata, ReplayGainMetadata{
		TrackGainDB: float64Pointer(-7.25),
		TrackPeak:   float64Pointer(0.987654),
		AlbumGainDB: float64Pointer(-6.5),
		AlbumPeak:   float64Pointer(1.012345),
	})
}

func float64Pointer(value float64) *float64 {
	return &value
}

func assertReplayGainMetadata(t *testing.T, got, want ReplayGainMetadata) {
	t.Helper()
	assertOptionalFloat64(t, "track gain", got.TrackGainDB, want.TrackGainDB)
	assertOptionalFloat64(t, "track peak", got.TrackPeak, want.TrackPeak)
	assertOptionalFloat64(t, "album gain", got.AlbumGainDB, want.AlbumGainDB)
	assertOptionalFloat64(t, "album peak", got.AlbumPeak, want.AlbumPeak)
}

func assertOptionalFloat64(t *testing.T, name string, got, want *float64) {
	t.Helper()
	if got == nil || want == nil {
		if got != want {
			t.Fatalf("%s = %v, want %v", name, got, want)
		}
		return
	}
	if *got != *want {
		t.Fatalf("%s = %v, want %v", name, *got, *want)
	}
}
