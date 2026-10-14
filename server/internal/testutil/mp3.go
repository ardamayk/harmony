package testutil

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"image"
	"image/color"
	"image/png"
	"strings"
)

const strictMP3AudioBase64 = `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
KiW/DJIDT3MIhVE=`

func StrictMP3Fixture() []byte {
	return StrictMP3FixtureWithID3Version(4)
}

func StrictMP3FixtureWithID3Version(version byte) []byte {
	audio, err := base64.StdEncoding.DecodeString(strictMP3AudioBase64)
	if err != nil {
		panic(err)
	}
	audio = makeVBRMP3(audio)
	return append(strictID3Tag(version), audio...)
}

func strictID3Tag(version byte) []byte {
	return id3TagBytes(version, strictID3Frames(version))
}

func strictID3Frames(version byte) [][]byte {
	names := strictID3FrameNames(version)
	frames := [][]byte{id3TextFrame(version, names[0], "MP3 Inspection Fixture")}
	frames = append(frames, structuredID3TextFrames(version, names[1], []string{"Primary Artist", "Guest Artist"})...)
	frames = append(frames, id3TextFrame(version, names[2], "Album Artist"), id3TextFrame(version, names[3], "Strict MP3 Import Tests"))
	frames = append(frames, id3TextFrame(version, names[4], "2/8"), id3TextFrame(version, names[5], "1/1"))
	frames = append(frames, structuredID3TextFrames(version, names[6], []string{"Electronic", "Ambient"})...)
	frames = append(frames, id3TextFrame(version, names[7], "2026-08-31"))
	for _, replayGain := range [][2]string{{"REPLAYGAIN_TRACK_GAIN", "-7.25 dB"}, {"REPLAYGAIN_TRACK_PEAK", "0.9123"}, {"REPLAYGAIN_ALBUM_GAIN", "-6.75 dB"}, {"REPLAYGAIN_ALBUM_PEAK", "0.9789"}} {
		frames = append(frames, id3UserTextFrame(version, replayGain[0], replayGain[1]))
	}
	frames = append(frames, id3ArtworkFrame(version, strictMP3Artwork()))
	return frames
}

func strictID3FrameNames(version byte) [8]string {
	if version == 2 {
		return [8]string{"TT2", "TP1", "TP2", "TAL", "TRK", "TPA", "TCO", "TYE"}
	}
	if version == 3 {
		return [8]string{"TIT2", "TPE1", "TPE2", "TALB", "TRCK", "TPOS", "TCON", "TYER"}
	}
	return [8]string{"TIT2", "TPE1", "TPE2", "TALB", "TRCK", "TPOS", "TCON", "TDRC"}
}

func structuredID3TextFrames(version byte, name string, values []string) [][]byte {
	if version == 4 {
		return [][]byte{id3TextFrame(version, name, strings.Join(values, "\x00"))}
	}
	if name == "TCO" || name == "TCON" {
		return [][]byte{id3TextFrame(version, name, strings.Join(values, "; "))}
	}
	frames := make([][]byte, 0, len(values))
	for _, value := range values {
		frames = append(frames, id3TextFrame(version, name, value))
	}
	return frames
}

func makeVBRMP3(audio []byte) []byte {
	var output bytes.Buffer
	for frameIndex, offset := 0, 0; offset < len(audio); frameIndex++ {
		frameSize := mpeg2Layer3FrameSize(audio[offset:])
		frame := append([]byte(nil), audio[offset:offset+frameSize]...)
		if frameIndex%2 == 1 {
			frame[2] = frame[2]&0x0f | 0x70
			frame = append(frame, make([]byte, 26)...)
		}
		output.Write(frame)
		offset += frameSize
	}
	return output.Bytes()
}

func mpeg2Layer3FrameSize(frame []byte) int {
	padding := int(frame[2]>>1) & 1
	return 72*48_000/22_050 + padding
}

func id3TagBytes(version byte, frames [][]byte) []byte {
	payload := bytes.Join(frames, nil)
	header := append([]byte("ID3"), version, 0, 0)
	header = append(header, syncSafeInt(len(payload))...)
	return append(header, payload...)
}

func id3TextFrame(version byte, name, value string) []byte {
	encoding := byte(0)
	if version == 4 {
		encoding = 3
	}
	return id3FrameBytes(version, name, append([]byte{encoding}, []byte(value)...))
}

func id3UserTextFrame(version byte, description, value string) []byte {
	encoding, name := byte(0), "TXXX"
	if version == 4 {
		encoding = 3
	}
	if version == 2 {
		name = "TXX"
	}
	payload := append([]byte{encoding}, []byte(description)...)
	payload = append(payload, 0)
	payload = append(payload, []byte(value)...)
	return id3FrameBytes(version, name, payload)
}

func id3ArtworkFrame(version byte, data []byte) []byte {
	payload := []byte{0}
	name := "APIC"
	if version == 2 {
		name = "PIC"
		payload = append(payload, []byte("PNG")...)
	} else {
		payload = append(payload, []byte("image/png")...)
		payload = append(payload, 0)
	}
	payload = append(payload, 3, 0)
	payload = append(payload, data...)
	return id3FrameBytes(version, name, payload)
}

func id3FrameBytes(version byte, name string, payload []byte) []byte {
	frame := []byte(name)
	if version == 2 {
		frame = append(frame, byte(len(payload)>>16), byte(len(payload)>>8), byte(len(payload)))
		return append(frame, payload...)
	}
	var size []byte
	if version == 4 {
		size = syncSafeInt(len(payload))
	} else {
		size = make([]byte, 4)
		binary.BigEndian.PutUint32(size, uint32(len(payload)))
	}
	frame = append(frame, size...)
	frame = append(frame, 0, 0)
	return append(frame, payload...)
}

func syncSafeInt(value int) []byte {
	return []byte{byte(value >> 21), byte(value >> 14 & 0x7f), byte(value >> 7 & 0x7f), byte(value & 0x7f)}
}

func strictMP3Artwork() []byte {
	artwork := image.NewNRGBA(image.Rect(0, 0, 32, 32))
	fill := color.NRGBA{R: 12, G: 98, B: 180, A: 255}
	for pixelY := 0; pixelY < 32; pixelY++ {
		for pixelX := 0; pixelX < 32; pixelX++ {
			artwork.SetNRGBA(pixelX, pixelY, fill)
		}
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, artwork); err != nil {
		panic(err)
	}
	return encoded.Bytes()
}

// StrictMP3FixtureWithExtraFrames appends the given ID3 frames to the strict
// fixture's tag so tests can add identifiers such as TSRC or UFID.
func StrictMP3FixtureWithExtraFrames(version byte, extraFrames ...[]byte) []byte {
	audio, err := base64.StdEncoding.DecodeString(strictMP3AudioBase64)
	if err != nil {
		panic(err)
	}
	audio = makeVBRMP3(audio)
	frames := strictID3Frames(version)
	frames = append(frames, extraFrames...)
	return append(id3TagBytes(version, frames), audio...)
}

// ID3TextFrame builds one text frame (for example TSRC).
func ID3TextFrame(version byte, name, value string) []byte {
	return id3TextFrame(version, name, value)
}

// ID3LyricsFrame builds a USLT (ULT for v2.2) frame: encoding, language,
// NUL-terminated descriptor, then the lyrics text.
func ID3LyricsFrame(version byte, language, description, text string) []byte {
	encoding, name := byte(0), "USLT"
	if version == 4 {
		encoding = 3
	}
	if version == 2 {
		name = "ULT"
	}
	payload := append([]byte{encoding}, []byte(language)...)
	payload = append(payload, []byte(description)...)
	payload = append(payload, 0)
	payload = append(payload, []byte(text)...)
	return id3FrameBytes(version, name, payload)
}
