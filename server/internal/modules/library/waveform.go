package library

import (
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"time"

	"golang.org/x/sync/singleflight"
)

// WaveformPeakCount is how many bars the Player Bar draws behind the seek
// bar; 400 peaks as bytes are about 1.5 KB of JSON.
const WaveformPeakCount = 400

const (
	waveformSampleRateHz      = 8000
	waveformGenerationTimeout = 60 * time.Second
	// Generation is CPU-bound; two at once keeps a library-wide burst from
	// starving imports and playback.
	waveformConcurrency = 2
	// A request waits this long for a fresh waveform before answering 202.
	waveformPendingAfter = 2 * time.Second
)

var (
	// ErrWaveformUnavailable means ffmpeg is missing, so no waveform can be made.
	ErrWaveformUnavailable = errors.New("waveform generation is unavailable")
	// ErrWaveformPending means generation is still running; ask again shortly.
	ErrWaveformPending = errors.New("waveform generation is pending")
)

// TrackWaveform is the API shape: normalized peaks in 0..255.
type TrackWaveform struct {
	TrackID   string `json:"trackId"`
	PeakCount int    `json:"peakCount"`
	Peaks     []int  `json:"peaks"`
}

// TrackWaveformPending is the 202 body while peaks are being generated.
type TrackWaveformPending struct {
	Status            string `json:"status"`
	RetryAfterSeconds int    `json:"retryAfterSeconds"`
}

// WaveformGenerator decodes a file and returns WaveformPeakCount peaks.
// durationMs comes from the library and lets the peaks be bucketed in one
// streaming pass; it may be zero for files whose length is unknown.
type WaveformGenerator func(ctx context.Context, path string, durationMs int) ([]byte, error)

// peakBucketer folds a stream of samples into a fixed number of bins,
// keeping the loudest absolute sample per bin.
type peakBucketer struct {
	count        int
	totalSamples int64
	seen         int64
	peaks        []int32
	// When the total is unknown every sample is kept and bucketed at the end.
	buffered []int16
}

func newPeakBucketer(count int, totalSamples int64) *peakBucketer {
	return &peakBucketer{
		count:        count,
		totalSamples: totalSamples,
		peaks:        make([]int32, count),
	}
}

func (b *peakBucketer) add(samples []int16) {
	if b.totalSamples <= 0 {
		b.buffered = append(b.buffered, samples...)
		return
	}
	for _, sample := range samples {
		bin := int(b.seen * int64(b.count) / b.totalSamples)
		if bin >= b.count {
			bin = b.count - 1
		}
		magnitude := int32(sample)
		if magnitude < 0 {
			magnitude = -magnitude
		}
		if magnitude > b.peaks[bin] {
			b.peaks[bin] = magnitude
		}
		b.seen++
	}
}

// finish normalizes the bins to 0..255 against the loudest bin. Silence
// yields all zeros rather than dividing by zero.
func (b *peakBucketer) finish() []byte {
	if b.totalSamples <= 0 && len(b.buffered) > 0 {
		b.totalSamples = int64(len(b.buffered))
		buffered := b.buffered
		b.buffered = nil
		b.add(buffered)
	}
	var loudest int32
	for _, peak := range b.peaks {
		if peak > loudest {
			loudest = peak
		}
	}
	out := make([]byte, b.count)
	if loudest == 0 {
		return out
	}
	for index, peak := range b.peaks {
		out[index] = byte(int64(peak) * 255 / int64(loudest))
	}
	return out
}

// bucketPeaks is the pure core: raw signed 16-bit little-endian PCM in,
// normalized peaks out. Exposed for tests; the generator streams through it.
func bucketPeaks(pcm []byte, count int, totalSamples int64) []byte {
	bucketer := newPeakBucketer(count, totalSamples)
	bucketer.add(decodePCM16(pcm))
	return bucketer.finish()
}

func decodePCM16(pcm []byte) []int16 {
	samples := make([]int16, len(pcm)/2)
	for index := range samples {
		samples[index] = int16(binary.LittleEndian.Uint16(pcm[index*2:]))
	}
	return samples
}

// FFmpegWaveformGenerator decodes the file with ffmpeg to mono 8 kHz PCM
// and buckets it on the fly, so memory stays flat however long the track.
func FFmpegWaveformGenerator(ctx context.Context, path string, durationMs int) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, waveformGenerationTimeout)
	defer cancel()
	command := exec.CommandContext(ctx, "ffmpeg",
		"-v", "error", "-nostdin", "-i", path,
		"-vn", "-ac", "1", "-ar", fmt.Sprint(waveformSampleRateHz),
		"-f", "s16le", "-")
	stdout, err := command.StdoutPipe()
	if err != nil {
		return nil, fmt.Errorf("waveform decoder pipe: %w", err)
	}
	errorOutput := newBoundedBuffer(FFMPEG_ERROR_OUTPUT_LIMIT_BYTES)
	command.Stderr = errorOutput
	if err := command.Start(); err != nil {
		return nil, fmt.Errorf("start waveform decoder: %w", err)
	}
	totalSamples := int64(durationMs) * waveformSampleRateHz / 1000
	bucketer := newPeakBucketer(WaveformPeakCount, totalSamples)
	readErr := readPCMStream(stdout, bucketer)
	waitErr := command.Wait()
	if ctx.Err() != nil {
		return nil, fmt.Errorf("waveform decode: %w", ctx.Err())
	}
	if readErr != nil {
		return nil, fmt.Errorf("read waveform samples: %w", readErr)
	}
	if waitErr != nil {
		return nil, commandFailure("decode waveform", waitErr, errorOutput)
	}
	return bucketer.finish(), nil
}

func readPCMStream(reader io.Reader, bucketer *peakBucketer) error {
	chunk := make([]byte, 16384)
	var carry byte
	hasCarry := false
	for {
		n, err := reader.Read(chunk)
		if n > 0 {
			data := chunk[:n]
			if hasCarry {
				data = append([]byte{carry}, data...)
				hasCarry = false
			}
			if len(data)%2 == 1 {
				carry = data[len(data)-1]
				hasCarry = true
				data = data[:len(data)-1]
			}
			bucketer.add(decodePCM16(data))
		}
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
	}
}

// WaveformSource is what the service needs to know about a Track's file.
type WaveformSource struct {
	FilePath   string
	DurationMs int
}

// WaveformRecord is a cached waveform plus the file identity it came from.
type WaveformRecord struct {
	Peaks            []byte
	SourceSizeBytes  int64
	SourceModifiedAt int64
}

type waveformRepository interface {
	GetTrackWaveformSource(ctx context.Context, trackID string) (WaveformSource, error)
	GetTrackWaveform(ctx context.Context, trackID string) (WaveformRecord, bool, error)
	PutTrackWaveform(ctx context.Context, trackID string, record WaveformRecord) error
}

// WaveformService serves cached peaks and generates missing ones once per
// Track at a time, answering "pending" when generation outlasts a request.
type WaveformService struct {
	repo         waveformRepository
	generate     WaveformGenerator
	available    bool
	pendingAfter time.Duration
	group        singleflight.Group
	semaphore    chan struct{}
}

func NewWaveformService(repo waveformRepository, generate WaveformGenerator, available bool) *WaveformService {
	return &WaveformService{
		repo:         repo,
		generate:     generate,
		available:    available,
		pendingAfter: waveformPendingAfter,
		semaphore:    make(chan struct{}, waveformConcurrency),
	}
}

func (s *WaveformService) Get(ctx context.Context, trackID string) (TrackWaveform, error) {
	if s == nil || !s.available {
		return TrackWaveform{}, ErrWaveformUnavailable
	}
	source, err := s.repo.GetTrackWaveformSource(ctx, trackID)
	if err != nil {
		return TrackWaveform{}, err
	}
	info, err := os.Stat(source.FilePath)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return TrackWaveform{}, ErrNotFound
		}
		return TrackWaveform{}, fmt.Errorf("stat waveform source: %w", err)
	}
	identity := WaveformRecord{SourceSizeBytes: info.Size(), SourceModifiedAt: info.ModTime().Unix()}
	cached, found, err := s.repo.GetTrackWaveform(ctx, trackID)
	if err != nil {
		return TrackWaveform{}, err
	}
	if found && cached.SourceSizeBytes == identity.SourceSizeBytes && cached.SourceModifiedAt == identity.SourceModifiedAt {
		return toTrackWaveform(trackID, cached.Peaks), nil
	}

	results := s.group.DoChan(trackID, func() (any, error) {
		// Generation outlives the request that started it, so it runs on a
		// background context and finishes for the next caller.
		s.semaphore <- struct{}{}
		defer func() { <-s.semaphore }()
		peaks, err := s.generate(context.Background(), source.FilePath, source.DurationMs)
		if err != nil {
			return nil, err
		}
		identity.Peaks = peaks
		if err := s.repo.PutTrackWaveform(context.Background(), trackID, identity); err != nil {
			return nil, err
		}
		return peaks, nil
	})
	select {
	case result := <-results:
		if result.Err != nil {
			return TrackWaveform{}, result.Err
		}
		peaks, _ := result.Val.([]byte)
		return toTrackWaveform(trackID, peaks), nil
	case <-time.After(s.pendingAfter):
		return TrackWaveform{}, ErrWaveformPending
	case <-ctx.Done():
		return TrackWaveform{}, ctx.Err()
	}
}

func toTrackWaveform(trackID string, peaks []byte) TrackWaveform {
	values := make([]int, len(peaks))
	for index, peak := range peaks {
		values[index] = int(peak)
	}
	return TrackWaveform{TrackID: trackID, PeakCount: len(values), Peaks: values}
}
