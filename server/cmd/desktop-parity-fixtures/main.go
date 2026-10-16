// Command desktop-parity-fixtures writes one Strict Import Profile audio
// fixture per supported Managed Import format so the Desktop Client parity
// test (desktop/core/tests/managed_import_parity.rs) can drive real
// imports through the native transport against a real Music Server.
//
// The fixtures are the same bytes the Music Server's own Go tests use: the
// generated MP3 and WAV builders from internal/testutil plus the checked-in
// FLAC, Ogg Vorbis, Opus, and M4A files under internal/modules/library/testdata.
// The command must run from the server module directory and is intended for
// test tooling only.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

const LIBRARY_TESTDATA_DIR = "internal/modules/library/testdata"

// Fixture describes one written file so the Rust test can match the
// server-detected format without parsing audio itself.
type Fixture struct {
	Filename string `json:"filename"`
	Format   string `json:"format"`
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "desktop-parity-fixtures:", err)
		os.Exit(1)
	}
}

func run() error {
	outputDir := flag.String("out", "", "directory that receives the fixture files and fixtures.json")
	flag.Parse()
	if *outputDir == "" {
		return fmt.Errorf("-out is required")
	}
	if err := os.MkdirAll(*outputDir, 0o755); err != nil {
		return fmt.Errorf("create output directory: %w", err)
	}

	fixtures := []Fixture{
		{Filename: "strict-import.flac", Format: "flac"},
		{Filename: "strict-import.mp3", Format: "mp3"},
		{Filename: "strict-import-aac.m4a", Format: "m4a"},
		{Filename: "strict-import.ogg", Format: "ogg"},
		{Filename: "strict-import.opus", Format: "opus"},
		{Filename: "strict-import.wav", Format: "wav"},
	}
	for _, fixture := range fixtures {
		bytes, err := fixtureBytes(fixture)
		if err != nil {
			return err
		}
		if err := os.WriteFile(filepath.Join(*outputDir, fixture.Filename), bytes, 0o644); err != nil {
			return fmt.Errorf("write %s: %w", fixture.Filename, err)
		}
	}

	manifest, err := json.MarshalIndent(fixtures, "", "  ")
	if err != nil {
		return fmt.Errorf("encode manifest: %w", err)
	}
	if err := os.WriteFile(filepath.Join(*outputDir, "fixtures.json"), manifest, 0o644); err != nil {
		return fmt.Errorf("write manifest: %w", err)
	}
	return nil
}

func fixtureBytes(fixture Fixture) ([]byte, error) {
	switch fixture.Format {
	case "mp3":
		return testutil.StrictMP3Fixture(), nil
	case "wav":
		return testutil.BuildWAV(testutil.WAVFixture{
			AudioFormat:  1,
			ChannelCount: 2,
			SampleRateHz: 48000,
			BitDepth:     24,
			PCMFrames:    4800,
			ID3Frames: []testutil.WAVID3Frame{
				testutil.WAVTextFrame("TIT2", "WAV Fixture"),
				testutil.WAVTextFrame("TPE1", "WAV Artist"),
				testutil.WAVTextFrame("TPE2", "WAV Album Artist"),
				testutil.WAVTextFrame("TALB", "WAV Strict Import"),
				testutil.WAVTextFrame("TRCK", "3/9"),
				testutil.WAVTextFrame("TPOS", "1/1"),
				testutil.WAVTextFrame("TCON", "Ambient"),
				testutil.WAVTextFrame("TDRC", "2026"),
				testutil.WAVAPICFrame(3, "image/png", "cover", testutil.WAVCoverPNG()),
			},
		}), nil
	default:
		bytes, err := os.ReadFile(filepath.Join(LIBRARY_TESTDATA_DIR, fixture.Filename))
		if err != nil {
			return nil, fmt.Errorf("read checked-in %s fixture: %w", fixture.Format, err)
		}
		return bytes, nil
	}
}
