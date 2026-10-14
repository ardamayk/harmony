package searchdata_test

import (
	"context"
	"reflect"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/searchdata"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func TestPrepareQueryPreservesSpellingAndWordBoundaries(t *testing.T) {
	for _, testCase := range testutil.LibrarySearchNormalizationCases() {
		t.Run(testCase.Input, func(t *testing.T) {
			got := searchdata.PrepareQuery(testCase.Input)
			if got.Faithful != testCase.Faithful || got.Folded != testCase.Folded || got.Compact != testCase.Compact {
				t.Fatalf("query %q: got %+v", testCase.Input, got)
			}
		})
	}
	if got := searchdata.PrepareQuery("... / !"); got.Folded != "" {
		t.Fatalf("punctuation query: %+v", got)
	}
	if !reflect.DeepEqual(searchdata.PrepareQuery("live remix radio edit").Versions, []string{"live", "remix", "radio edit"}) {
		t.Fatal("query version words must be retained and recognized anywhere")
	}
}

func TestStoredNamesUseTheQueryNormalizationRules(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	seedLibrary(t, database)
	for _, testCase := range testutil.LibrarySearchNormalizationCases() {
		t.Run(testCase.Input, func(t *testing.T) {
			execute(t, database, `UPDATE tracks SET title = ? WHERE id = 'track'`, testCase.Input)
			document, err := searchdata.NewRepository(database).GetDocument(context.Background(), "track", "track", "listener")
			if err != nil {
				t.Fatal(err)
			}
			text := document.Fields[0].Text
			if text.Original != testCase.Input || text.Faithful != testCase.Faithful || text.Folded != testCase.Folded || text.Compact != testCase.Compact {
				t.Fatalf("stored normalization: %+v", text)
			}
		})
	}
}
