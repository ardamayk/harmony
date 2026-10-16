package library

import (
	"bytes"
	"context"
	"fmt"
	"image"
	"image/jpeg"

	"golang.org/x/image/draw"
)

// CoverSizes are the longest-edge sizes a scaled-down cover can be asked for.
// Clients pick the one at or above the displayed size times the pixel ratio,
// so a 250px card loads a 512px cover instead of a 3000px original.
var CoverSizes = map[int]bool{64: true, 128: true, 256: true, 512: true, 1024: true}

const coverVariantJPEGQuality = 82

// albumCoverVariant returns the cover scaled to fit size, generating and
// storing it on first use. A cover already within size is returned as is.
func (s *Service) albumCoverVariant(ctx context.Context, albumID string, size int) (AlbumCover, error) {
	original, err := s.repo.GetAlbumCoverSource(ctx, albumID)
	if err != nil {
		return AlbumCover{}, err
	}
	stored, found, err := s.repo.GetAlbumCoverVariant(ctx, albumID, size)
	if err != nil {
		return AlbumCover{}, err
	}
	if found && stored.SourceSHA256 == original.SourceSHA256 {
		return stored, nil
	}
	config, _, err := image.DecodeConfig(bytes.NewReader(original.Data))
	if err != nil || max(config.Width, config.Height) <= size {
		// Unreadable or already small enough: the original is the answer.
		return original, nil
	}
	scaled, err := scaleCover(original.Data, size)
	if err != nil {
		return original, nil
	}
	variant := AlbumCover{MediaType: "image/jpeg", Data: scaled, SourceSHA256: original.SourceSHA256}
	if err := s.repo.PutAlbumCoverVariant(ctx, albumID, size, variant); err != nil {
		return AlbumCover{}, err
	}
	return variant, nil
}

// scaleCover fits the image into size×size, keeping its aspect ratio, and
// encodes it as JPEG.
func scaleCover(data []byte, size int) ([]byte, error) {
	source, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return nil, fmt.Errorf("decode cover: %w", err)
	}
	bounds := source.Bounds()
	width, height := bounds.Dx(), bounds.Dy()
	if width >= height {
		height = max(1, height*size/width)
		width = size
	} else {
		width = max(1, width*size/height)
		height = size
	}
	target := image.NewRGBA(image.Rect(0, 0, width, height))
	draw.CatmullRom.Scale(target, target.Bounds(), source, bounds, draw.Src, nil)
	var encoded bytes.Buffer
	if err := jpeg.Encode(&encoded, target, &jpeg.Options{Quality: coverVariantJPEGQuality}); err != nil {
		return nil, fmt.Errorf("encode cover: %w", err)
	}
	return encoded.Bytes(), nil
}
