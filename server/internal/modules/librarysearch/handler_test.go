package librarysearch_test

import (
	"database/sql"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"testing"

	apigen "github.com/ardam/navidrome-replacement/server/internal/api/gen"
	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/ardam/navidrome-replacement/server/internal/modules/librarysearch"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

// The fixture is one coherent library; every test reads it through the HTTP
// boundary and asserts identities, order, and group membership only.
func newSearchServer(t *testing.T) (http.Handler, *sql.DB) {
	t.Helper()
	database := testutil.OpenMigratedDB(t)
	testutil.SeedLibrarySearchFixture(t, database)
	router := chi.NewRouter()
	librarysearch.NewModule(database).RegisterRoutes(router)
	return router, database
}

func execute(t *testing.T, database *sql.DB, statement string, arguments ...any) {
	t.Helper()
	if _, err := database.Exec(statement, arguments...); err != nil {
		t.Fatalf("%s: %v", statement, err)
	}
}

func search(t *testing.T, handler http.Handler, query string) apigen.LibrarySearchResponse {
	t.Helper()
	response := testutil.ServeContractRequest(t, handler, testutil.ContractRequest{Method: http.MethodGet, Path: "/api/v1/library/search?q=" + url.QueryEscape(query)})
	if response.Code != http.StatusOK {
		t.Fatalf("search %q status = %d, body = %s", query, response.Code, response.Body.String())
	}
	var result apigen.LibrarySearchResponse
	testutil.DecodeJSON(t, response, &result)
	return result
}

func ids(results []apigen.LibrarySearchResult) []string {
	out := make([]string, 0, len(results))
	for _, result := range results {
		out = append(out, result.Id)
	}
	return out
}

func matches(results []apigen.LibrarySearchResult) []string {
	out := make([]string, 0, len(results))
	for _, result := range results {
		out = append(out, string(result.Match))
	}
	return out
}

func assertIDs(t *testing.T, label string, results []apigen.LibrarySearchResult, expected ...string) {
	t.Helper()
	actual := ids(results)
	if len(actual) != len(expected) {
		t.Fatalf("%s = %v, want %v", label, actual, expected)
	}
	for index := range expected {
		if actual[index] != expected[index] {
			t.Fatalf("%s = %v, want %v", label, actual, expected)
		}
	}
}

func assertBestMatch(t *testing.T, result apigen.LibrarySearchResponse, kind apigen.LibrarySearchResultType, id string) {
	t.Helper()
	if result.BestMatch == nil {
		t.Fatalf("best match missing, want %s %q", kind, id)
	}
	if result.BestMatch.Type != kind || result.BestMatch.Id != id || result.BestMatch.Match != apigen.Direct {
		t.Fatalf("best match = %+v, want direct %s %q", *result.BestMatch, kind, id)
	}
	group := qualityGroups(result)[string(kind)]
	if len(group) == 0 || len(group) > 5 || group[0].Id != id || group[0].Match != apigen.Direct {
		t.Fatalf("Best Match must lead its capped category: %+v", group)
	}
}

func TestRankingCasesFromTheSharedFixture(t *testing.T) {
	handler, _ := newSearchServer(t)
	for _, item := range testutil.LibrarySearchRankingCases() {
		result := search(t, handler, item.Query)
		if item.BestMatchID == "" {
			if result.BestMatch != nil {
				t.Fatalf("%q: no Best Match expected, got %+v", item.Query, *result.BestMatch)
			}
		} else {
			assertBestMatch(t, result, apigen.LibrarySearchResultType(item.BestMatchKind), item.BestMatchID)
		}
		assertIDs(t, item.Query+" tracks", result.Tracks, item.Tracks...)
		assertIDs(t, item.Query+" albums", result.Albums, item.Albums...)
		assertIDs(t, item.Query+" artists", result.Artists, item.Artists...)
		assertIDs(t, item.Query+" genres", result.Genres, item.Genres...)
		assertIDs(t, item.Query+" playlists", result.Playlists, item.Playlists...)
	}
}

func TestResultsCarryDisplayMetadataAndMatchLabels(t *testing.T) {
	handler, _ := newSearchServer(t)
	result := search(t, handler, "Blinding Lights")
	if result.BestMatch.Name != "Blinding Lights" || result.BestMatch.Album == nil || result.BestMatch.Album.Name != "After Hours" ||
		result.BestMatch.Artists == nil || len(*result.BestMatch.Artists) != 1 || (*result.BestMatch.Artists)[0].Name != "The Weeknd" {
		t.Fatalf("display metadata = %+v", *result.BestMatch)
	}
	assertIDs(t, "no Album expansion", result.Albums)
	assertIDs(t, "no Artist expansion", result.Artists)
	result = search(t, handler, "blue")
	if result.Albums[0].Match != apigen.Direct {
		t.Fatalf("Lemonade Blue must match its own title directly: %+v", result.Albums)
	}
	result = search(t, handler, "the weeknd")
	assertIDs(t, "credit-only Album excluded", result.Albums)
}

func TestCorrectedResultsAreLabeled(t *testing.T) {
	handler, _ := newSearchServer(t)
	result := search(t, handler, "blidning lights")
	if matches(result.Tracks)[0] != "corrected" {
		t.Fatalf("corrected label missing: %v", matches(result.Tracks))
	}
}

func TestPunctuationAndTurkishLetterQueries(t *testing.T) {
	handler, _ := newSearchServer(t)
	for query, artist := range map[string]string{"sebnem ferah": "sebnem", "ŞEBNEM FERAH": "sebnem", "ac dc": "acdc", "acdc": "acdc", "AC/DC": "acdc"} {
		assertBestMatch(t, search(t, handler, query), apigen.LibrarySearchResultTypeArtist, artist)
	}
	for _, query := range []string{"dont stop", "don't stop", "don’t stop"} {
		assertBestMatch(t, search(t, handler, query), apigen.LibrarySearchResultTypeTrack, "dont-stop")
	}
}

func TestSharedNormalizationFixturesFindTheirRecords(t *testing.T) {
	handler, database := newSearchServer(t)
	cases := testutil.LibrarySearchNormalizationCases()
	for index, item := range cases {
		if item.Folded == "" {
			continue
		}
		execute(t, database, `INSERT INTO playlists(id,user_id,name) VALUES (?,?,?)`, "case-"+string(rune('a'+index)), auth.DefaultUserID, item.Input)
	}
	for index, item := range cases {
		if item.Folded == "" {
			continue
		}
		result := search(t, handler, item.Input)
		id := "case-" + string(rune('a'+index))
		if (result.BestMatch == nil || result.BestMatch.Id != id) && !slices.Contains(ids(result.Playlists), id) {
			t.Fatalf("%q: best match = %+v, playlists = %v", item.Input, result.BestMatch, ids(result.Playlists))
		}
	}
}

func TestArtistResultsRequireTheirOwnNames(t *testing.T) {
	handler, _ := newSearchServer(t)
	result := search(t, handler, "lights")
	assertIDs(t, "Artist name matches", result.Artists, "lights", "northern-lights")
	for _, artist := range result.Artists {
		if artist.Match != apigen.Direct {
			t.Fatalf("Artist must match directly: %+v", artist)
		}
	}
}

func TestInvalidQueriesAndFailuresAreErrors(t *testing.T) {
	handler, database := newSearchServer(t)
	for _, path := range []string{"/api/v1/library/search", "/api/v1/library/search?q=", "/api/v1/library/search?q=%20%20", "/api/v1/library/search?q=...%20/%20!", "/api/v1/library/search?q=" + strings.Repeat("a", 201)} {
		testutil.AssertErrorCode(t, testutil.ServeRequest(t, handler, http.MethodGet, path, nil, nil), http.StatusBadRequest, "bad_request")
	}
	if err := database.Close(); err != nil {
		t.Fatal(err)
	}
	response := testutil.ServeRequest(t, handler, http.MethodGet, "/api/v1/library/search?q=blinding", nil, nil)
	testutil.AssertErrorCode(t, response, http.StatusInternalServerError, "internal_error")
}

func TestSearchReflectsLibraryMutations(t *testing.T) {
	handler, database := newSearchServer(t)
	execute(t, database, `UPDATE tracks SET title = 'Storm' WHERE id = 'rain'`)
	execute(t, database, `UPDATE artists SET name = 'Abel' WHERE id = 'weeknd'`)
	execute(t, database, `DELETE FROM tracks WHERE id = 'u'`)
	execute(t, database, `UPDATE tracks SET is_pending_commit = 1 WHERE id = 'sunflower'`)
	assertBestMatch(t, search(t, handler, "storm"), apigen.LibrarySearchResultTypeTrack, "rain")
	assertBestMatch(t, search(t, handler, "abel"), apigen.LibrarySearchResultTypeArtist, "weeknd")
	for _, query := range []string{"rain", "u", "sunflower", "the weeknd"} {
		result := search(t, handler, query)
		if result.BestMatch != nil {
			t.Fatalf("%q must no longer match its previous record: %+v", query, *result.BestMatch)
		}
	}
	result := search(t, handler, "blinding lights")
	if (*result.BestMatch.Artists)[0].Name != "Abel" {
		t.Fatalf("stale credit in display metadata: %+v", *result.BestMatch)
	}
}
