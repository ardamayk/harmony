package managedimport

import "github.com/ardam/navidrome-replacement/server/internal/modules/library"

const (
	TRACK_REPLACEMENT_CONFIRMATION_HEADER = "X-Track-Replacement"
	MAX_TRACK_REPLACEMENT_BODY_BYTES      = 4 * 1024
	ERROR_CODE_ALBUM_POSITION_CONFLICT    = "album_position_conflict"
	ERROR_CODE_ALBUM_ARTWORK_CONFLICT     = "album_artwork_conflict"
	REPLACEMENT_ARTWORK_MODE_EXISTING     = "existing"
	REPLACEMENT_ARTWORK_MODE_CREATE       = "create"
	REPLACEMENT_ARTWORK_MODE_REPLACE      = "replace"
)

type replacementPhase string

const (
	REPLACEMENT_PHASE_PREPARED           replacementPhase = "prepared"
	REPLACEMENT_PHASE_PLACED             replacementPhase = "placed"
	REPLACEMENT_PHASE_VERIFIED           replacementPhase = "verified"
	REPLACEMENT_PHASE_SWAPPED            replacementPhase = "swapped"
	REPLACEMENT_PHASE_DATABASE_COMMITTED replacementPhase = "database_committed"
	REPLACEMENT_PHASE_COMPLETED          replacementPhase = "completed"
	REPLACEMENT_PHASE_ROLLED_BACK        replacementPhase = "rolled_back"
)

// TrackReplacementPreview makes every consequence of a Track Replacement visible before confirmation.
type TrackReplacementPreview struct {
	TrackID             string                           `json:"trackId"`
	TrackTitle          string                           `json:"trackTitle"`
	SourceFormat        TrackReplacementFieldDiff        `json:"sourceFormat"`
	TechnicalProperties []TrackReplacementFieldDiff      `json:"technicalProperties"`
	Metadata            []TrackReplacementFieldDiff      `json:"metadata"`
	Library             TrackReplacementLibraryChange    `json:"library"`
	Artwork             TrackReplacementArtworkChange    `json:"artwork"`
	CanonicalPath       TrackReplacementFieldDiff        `json:"canonicalPath"`
	OldFile             TrackReplacementFileDeletion     `json:"oldFile"`
	PlaylistReferences  []TrackDeletionPlaylistReference `json:"playlistReferences"`
	QueueReferences     []TrackDeletionQueueReference    `json:"queueReferences"`
	ConfirmationToken   string                           `json:"confirmationToken"`
}

type TrackReplacementFieldDiff struct {
	Field       string `json:"field"`
	Current     string `json:"current"`
	Replacement string `json:"replacement"`
	IsChanged   bool   `json:"isChanged"`
}

type TrackReplacementLibraryChange struct {
	CurrentAlbumID      string   `json:"currentAlbumId"`
	ReplacementAlbumID  string   `json:"replacementAlbumId,omitempty"`
	MovesAlbum          bool     `json:"movesAlbum"`
	CreatesAlbum        bool     `json:"createsAlbum"`
	RemovesEmptyAlbum   bool     `json:"removesEmptyAlbum"`
	RemovesEmptyArtists []string `json:"removesEmptyArtists"`
	CreatesArtists      []string `json:"createsArtists"`
	CreatesGenres       []string `json:"createsGenres"`
}

type TrackReplacementArtworkChange struct {
	CurrentMediaType     string `json:"currentMediaType"`
	CurrentSHA256        string `json:"currentSha256"`
	ReplacementMediaType string `json:"replacementMediaType"`
	ReplacementSHA256    string `json:"replacementSha256"`
	IsChanged            bool   `json:"isChanged"`
	ReplacesAlbumArtwork bool   `json:"replacesAlbumArtwork"`
}

type TrackReplacementFileDeletion struct {
	Path      string `json:"path"`
	SizeBytes int64  `json:"sizeBytes"`
}

type TrackReplacementConfirmation struct {
	Revision          int    `json:"revision"`
	ConfirmationToken string `json:"confirmationToken"`
}

type TrackReplacementResult struct {
	JobID        string       `json:"jobId"`
	Status       ImportStatus `json:"status"`
	Revision     int          `json:"revision"`
	TrackID      string       `json:"trackId"`
	DeletedFiles int          `json:"deletedFiles"`
}

// replacementJournal records every durable phase of a Track Replacement so that a crash at any point
// leaves either the previous managed file or the verified replacement fully usable.
type replacementJournal struct {
	ID                    string
	JobID                 string
	TrackID               string
	Phase                 replacementPhase
	StagedFilePath        string
	PendingAudioPath      string
	AudioFilePath         string
	PreviousAudioPath     string
	RetiredAudioPath      string
	AudioSHA256           string
	PreviousAudioSHA256   string
	ArtworkMode           string
	PendingArtworkPath    string
	ArtworkFilePath       string
	PreviousArtworkPath   string
	RetiredArtworkPath    string
	ArtworkSHA256         string
	PreviousArtworkSHA256 string
	IsArtworkCreated      bool
	PreviousAlbumID       string
	RecoveryReason        string
}

// replacementTarget is the current state of the Managed Track a Track Replacement will overwrite.
type replacementTarget struct {
	TrackID        string
	Title          string
	Artists        []string
	AlbumArtists   []string
	Album          string
	Year           int
	Genres         []string
	TrackNo        int
	TrackTotal     int
	DiscNo         int
	DiscTotal      int
	DurationMs     int
	Format         string
	Container      string
	Codec          string
	SampleRateHz   int
	ChannelCount   int
	BitDepth       int
	BitrateKbps    int
	SizeBytes      int64
	AlbumID        string
	AlbumKey       string
	FilePath       string
	RelativePath   string
	ContentSHA256  string
	TrackRevision  int
	SourceRevision int
	ArtworkPath    string
	ArtworkSHA256  string
	ArtworkType    string
	IsSoleTrack    bool
	Playlists      []TrackDeletionPlaylistReference
	Queues         []TrackDeletionQueueReference
}

type replacementCommitData struct {
	Job        importJob
	Target     replacementTarget
	Identity   commitIdentity
	Placement  replacementPlacement
	Inspection library.MediaInspection
	AlbumKey   string
}
