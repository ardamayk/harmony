package library

import (
	"encoding/json"
	"regexp"
	"sort"
	"strings"
)

var genreDelimiterPattern = regexp.MustCompile(`[;/|,]+`)

func splitGenres(raw string) []string {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil
	}
	parts := genreDelimiterPattern.Split(raw, -1)
	seen := make(map[string]struct{}, len(parts))
	out := make([]string, 0, len(parts))
	for _, part := range parts {
		part = strings.TrimSpace(part)
		if part == "" {
			continue
		}
		key := strings.ToLower(part)
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		out = append(out, part)
	}
	return out
}

// splitGenreTagValues expands raw GENRE tag values into individual Genres.
// Taggers record multiple Genres either as repeated tags or as one tag holding
// a delimited list ("Pop, Rock", "Symphonic Metal; Gothic Metal"), so every
// Media Inspector runs both forms through the same delimiters the Legacy
// scanner uses. Input order is kept and duplicates are dropped.
func splitGenreTagValues(values []string) []string {
	seen := make(map[string]struct{}, len(values))
	out := make([]string, 0, len(values))
	for _, value := range values {
		for _, genre := range splitGenres(value) {
			key := strings.ToLower(genre)
			if _, ok := seen[key]; ok {
				continue
			}
			seen[key] = struct{}{}
			out = append(out, genre)
		}
	}
	return out
}

func mergeGenres(genres ...[]string) []string {
	seen := make(map[string]string)
	for _, list := range genres {
		for _, genre := range list {
			genre = strings.TrimSpace(genre)
			if genre == "" {
				continue
			}
			key := strings.ToLower(genre)
			if _, ok := seen[key]; !ok {
				seen[key] = genre
			}
		}
	}
	out := make([]string, 0, len(seen))
	for _, genre := range seen {
		out = append(out, genre)
	}
	sort.Slice(out, func(i, j int) bool {
		return strings.ToLower(out[i]) < strings.ToLower(out[j])
	})
	return out
}

func decodeGenres(raw string) []string {
	raw = strings.TrimSpace(raw)
	if raw == "" || raw == "[]" {
		return nil
	}
	var genres []string
	if err := json.Unmarshal([]byte(raw), &genres); err != nil {
		return splitGenres(raw)
	}
	return mergeGenres(genres)
}
