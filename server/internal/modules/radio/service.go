package radio

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"strings"
)

// Service owns saved-station rules: validation of stream and link URLs,
// Radio Browser imports that update an existing station instead of
// duplicating it, and the last now-playing metadata.
type Service struct {
	repo *Repository
}

func NewService(repo *Repository) *Service {
	return &Service{repo: repo}
}

func (s *Service) ListStations(ctx context.Context, userID string) (StationList, error) {
	items, err := s.repo.List(ctx, userID)
	if err != nil {
		return StationList{}, err
	}
	return StationList{Items: items, Total: len(items)}, nil
}

func (s *Service) GetStation(ctx context.Context, userID, stationID string) (Station, error) {
	return s.repo.Get(ctx, userID, stationID)
}

func (s *Service) CreateStation(ctx context.Context, userID string, input StationCreate) (Station, error) {
	input.Name = strings.TrimSpace(input.Name)
	input.StreamURL = strings.TrimSpace(input.StreamURL)
	input.HomepageURL = strings.TrimSpace(input.HomepageURL)
	input.FaviconURL = strings.TrimSpace(input.FaviconURL)
	input.Country = strings.TrimSpace(input.Country)
	input.Language = strings.TrimSpace(input.Language)
	input.Codec = strings.TrimSpace(input.Codec)
	input.Source = strings.TrimSpace(input.Source)
	input.ExternalID = strings.TrimSpace(input.ExternalID)
	if err := validateStation(input.Name, input.StreamURL, input.HomepageURL, input.FaviconURL); err != nil {
		return Station{}, err
	}
	if input.Source == "" {
		input.Source = ManualSource
	}
	id, err := s.repo.Insert(ctx, userID, input)
	if err != nil {
		return Station{}, err
	}
	return s.repo.Get(ctx, userID, id)
}

func (s *Service) ImportStation(ctx context.Context, userID string, result SearchResult) (Station, error) {
	input := StationCreate{
		Name:        result.Name,
		StreamURL:   result.StreamURL,
		HomepageURL: result.HomepageURL,
		FaviconURL:  result.FaviconURL,
		Country:     result.Country,
		Language:    result.Language,
		Tags:        result.Tags,
		Codec:       result.Codec,
		Bitrate:     result.Bitrate,
		Source:      RadioBrowserSource,
		ExternalID:  result.StationUUID,
	}
	existing, err := s.repo.GetByExternalID(ctx, userID, input.Source, input.ExternalID)
	if errors.Is(err, ErrNotFound) {
		return s.CreateStation(ctx, userID, input)
	}
	if err != nil {
		return Station{}, err
	}
	return s.UpdateStation(ctx, userID, existing.ID, StationPatch{
		Name:        &input.Name,
		StreamURL:   &input.StreamURL,
		HomepageURL: &input.HomepageURL,
		FaviconURL:  &input.FaviconURL,
		Country:     &input.Country,
		Language:    &input.Language,
		Tags:        input.Tags,
		Codec:       &input.Codec,
		Bitrate:     &input.Bitrate,
	})
}

func (s *Service) UpdateStation(ctx context.Context, userID, stationID string, patch StationPatch) (Station, error) {
	current, err := s.repo.Get(ctx, userID, stationID)
	if err != nil {
		return Station{}, err
	}
	applyTrimmed(&current.Name, patch.Name)
	applyTrimmed(&current.StreamURL, patch.StreamURL)
	applyTrimmed(&current.HomepageURL, patch.HomepageURL)
	applyTrimmed(&current.FaviconURL, patch.FaviconURL)
	applyTrimmed(&current.Country, patch.Country)
	applyTrimmed(&current.Language, patch.Language)
	applyTrimmed(&current.Codec, patch.Codec)
	if patch.Tags != nil {
		current.Tags = patch.Tags
	}
	if patch.Bitrate != nil {
		current.Bitrate = *patch.Bitrate
	}
	if patch.IsFavorite != nil {
		current.IsFavorite = *patch.IsFavorite
	}
	if patch.Position != nil {
		current.Position = *patch.Position
	}
	if err := validateStation(current.Name, current.StreamURL, current.HomepageURL, current.FaviconURL); err != nil {
		return Station{}, err
	}
	if err := s.repo.Update(ctx, userID, current); err != nil {
		return Station{}, err
	}
	return s.repo.Get(ctx, userID, stationID)
}

func (s *Service) DeleteStation(ctx context.Context, userID, stationID string) error {
	return s.repo.Delete(ctx, userID, stationID)
}

func (s *Service) UpdateNowPlaying(ctx context.Context, stationID string, now NowPlaying) error {
	return s.repo.UpdateNowPlaying(ctx, stationID, now)
}

func applyTrimmed(target *string, value *string) {
	if value != nil {
		*target = strings.TrimSpace(*value)
	}
}

func validateStation(name, streamURL, homepageURL, faviconURL string) error {
	if name == "" {
		return fmt.Errorf("station name is required")
	}
	if streamURL == "" {
		return fmt.Errorf("streamUrl is required")
	}
	if err := ValidateStreamURL(streamURL); err != nil {
		return fmt.Errorf("invalid streamUrl: %w", err)
	}
	if err := validateOptionalHTTPURL("homepageUrl", homepageURL); err != nil {
		return err
	}
	return validateOptionalHTTPURL("faviconUrl", faviconURL)
}

func validateOptionalHTTPURL(field, rawURL string) error {
	if rawURL == "" {
		return nil
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return fmt.Errorf("invalid %s: %w", field, err)
	}
	if (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Hostname() == "" {
		return fmt.Errorf("invalid %s: must be an absolute HTTP(S) URI", field)
	}
	return nil
}
