package library

import "testing"

func TestSplitGenres(t *testing.T) {
	got := splitGenres("Alternative Pop; Contemporary R&B / Dance-Pop")
	want := []string{"Alternative Pop", "Contemporary R&B", "Dance-Pop"}
	if len(got) != len(want) {
		t.Fatalf("split = %#v, want %#v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("split[%d] = %q, want %q", i, got[i], want[i])
		}
	}
}

func TestMergeGenresDedupesCaseInsensitive(t *testing.T) {
	got := mergeGenres([]string{"Pop", "pop"}, []string{"Rock"})
	if len(got) != 2 {
		t.Fatalf("merge = %#v, want 2 genres", got)
	}
}

func TestDecodeGenresLegacyString(t *testing.T) {
	got := decodeGenres("Synthpop; Pop")
	if len(got) != 2 {
		t.Fatalf("decode legacy = %#v, want 2 genres", got)
	}
	seen := map[string]bool{got[0]: true, got[1]: true}
	if !seen["Pop"] || !seen["Synthpop"] {
		t.Fatalf("decode legacy = %#v", got)
	}
}

func TestSplitGenreTagValuesUnifiesRepeatedAndDelimitedTags(t *testing.T) {
	cases := []struct {
		name   string
		values []string
		want   []string
	}{
		{"comma delimited single tag", []string{"Pop, Rock"}, []string{"Pop", "Rock"}},
		{"semicolon delimited single tag", []string{"Symphonic Metal; Gothic Metal; Power Metal"}, []string{"Symphonic Metal", "Gothic Metal", "Power Metal"}},
		{"repeated tags", []string{"Pop", "Rock"}, []string{"Pop", "Rock"}},
		{"mixed delimiters", []string{"R&B; Pop/Rock|Live, Bootleg"}, []string{"R&B", "Pop", "Rock", "Live", "Bootleg"}},
		{"single Genre is left whole", []string{"Symphonic Metal"}, []string{"Symphonic Metal"}},
		{"duplicates across tags collapse", []string{"Pop, Rock", "rock"}, []string{"Pop", "Rock"}},
		{"delimiters only", []string{" ; , "}, []string{}},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			got := splitGenreTagValues(testCase.values)
			if len(got) != len(testCase.want) {
				t.Fatalf("splitGenreTagValues(%#v) = %#v, want %#v", testCase.values, got, testCase.want)
			}
			for index, genre := range testCase.want {
				if got[index] != genre {
					t.Fatalf("splitGenreTagValues(%#v) = %#v, want %#v", testCase.values, got, testCase.want)
				}
			}
		})
	}
}

func TestInspectVorbisNamesSplitsDelimitedGenreTag(t *testing.T) {
	names, err := inspectVorbisNames(map[string][]string{
		"TITLE":       {"Welcome to New York"},
		"ARTIST":      {"Taylor Swift"},
		"ALBUMARTIST": {"Taylor Swift"},
		"ALBUM":       {"1989"},
		"GENRE":       {"Pop, Rock"},
	})
	if err != nil {
		t.Fatalf("inspect Vorbis names: %v", err)
	}
	if len(names.Genres) != 2 || names.Genres[0] != "Pop" || names.Genres[1] != "Rock" {
		t.Fatalf("Genres = %#v, want [Pop Rock]", names.Genres)
	}
}

func TestInspectVorbisNamesAcceptsEmptyOptionalGenre(t *testing.T) {
	names, err := inspectVorbisNames(map[string][]string{
		"TITLE":       {"Track"},
		"ARTIST":      {"Artist"},
		"ALBUMARTIST": {"Artist"},
		"ALBUM":       {"Album"},
		"GENRE":       {";"},
	})
	if err != nil || len(names.Genres) != 0 {
		t.Fatalf("optional empty GENRE: %+v, %v", names.Genres, err)
	}
}
