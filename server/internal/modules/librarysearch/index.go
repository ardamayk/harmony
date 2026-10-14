package librarysearch

import (
	"strings"
	"unicode"

	"github.com/ardam/navidrome-replacement/server/internal/searchdata"
)

// preparedText is one stored representation with its word lists computed once
// per load; queries never normalize library names again.
type preparedText struct {
	searchdata.Text
	faithfulWords []string
	foldedWords   []string
	compactWords  []string
}

type fieldRef struct {
	name      string
	relatedID string
	text      *preparedText
}

type entity struct {
	kind    string
	id      string
	primary *preparedText
	related []fieldRef
}

func (e *entity) fields(name string) []fieldRef {
	var out []fieldRef
	for _, field := range e.related {
		if field.name == name {
			out = append(out, field)
		}
	}
	return out
}

// Index is a visibility-filtered snapshot of every searchable entity. Candidate
// collection covers the whole eligible library so ranking, deduplication, and
// group limits are applied globally rather than to truncated pages.
type Index struct {
	entities []*entity
	byKey    map[entityKey]*entity
}

type entityKey struct{ kind, id string }

// Result types in group order, matching the prepared-data kinds.
const (
	KIND_TRACK    = "track"
	KIND_ALBUM    = "album"
	KIND_ARTIST   = "artist"
	KIND_GENRE    = "genre"
	KIND_PLAYLIST = "playlist"
)

// relatedKinds identifies credit fields used to break otherwise equal name matches.
var relatedKinds = map[string]string{"album": KIND_ALBUM, "track_artist": KIND_ARTIST, "album_artist": KIND_ARTIST}

// splitFaithful keeps accents and inner punctuation (don't, ac/dc) as spelling
// evidence while ignoring enclosing punctuation such as the parentheses of a
// version section.
func splitFaithful(faithful string) []string {
	words := strings.Fields(faithful)
	out := words[:0]
	for _, word := range words {
		if word = strings.TrimFunc(word, func(r rune) bool { return !unicode.IsLetter(r) && !unicode.IsDigit(r) }); word != "" {
			out = append(out, word)
		}
	}
	return out
}
