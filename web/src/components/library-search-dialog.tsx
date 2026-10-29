import {
	ApiError,
	EXTERNAL_SOURCE_CAPABILITY,
	type ExternalTrack,
	type LibrarySearchResult,
} from "@repo/api-client";
import { AlbumArt, startTiming, toast, usePlayback } from "@repo/ui";
import {
	keepPreviousData,
	useQuery,
	useQueryClient,
} from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
	ArrowDown,
	ArrowUp,
	CornerDownLeft,
	Disc3,
	Globe,
	Library,
	ListMusic,
	ListPlus,
	LoaderCircle,
	Music2,
	Search,
	Tags,
	Users,
} from "lucide-react";
import {
	useCallback,
	useEffect,
	useId,
	useMemo,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import {
	Dialog,
	DialogClose,
	DialogContent,
	DialogTitle,
} from "#/components/ui/dialog";
import { useDebouncedValue } from "#/hooks/use-debounced-value";
import { useReturnFocus } from "#/hooks/use-return-focus";
import {
	useExternalSourceName,
	useServerCapability,
} from "#/hooks/use-server-capability";
import { apiClient } from "#/lib/api";
import {
	EXTERNAL_PLAYBACK_STALE_MS,
	externalPlaybackQueryOptions,
} from "#/lib/external-playback-query";
import { getTrackArtistName } from "#/lib/library-display";
import { libraryQueryKeys } from "#/lib/library-query-keys";
import { cn } from "#/lib/utils";

const RESULTS_PER_GROUP = 5;
const SEARCH_DEBOUNCE_MS = 200;
/** Each yt-dlp search takes about a second, so typing waits longer for one. */
const EXTERNAL_SEARCH_DEBOUNCE_MS = 500;

/** A short reason for a failed External Track play, from the Music Server's error code. */
function externalPlaybackFailureMessage(error: unknown): string {
	const code = error instanceof ApiError ? error.body.code : undefined;
	switch (code) {
		case "external_track_unplayable":
			return "This track can't be played here";
		case "external_source_timeout":
			return "The External Source took too long";
		default:
			return "Couldn't play this track";
	}
}
/** A pointer resting this long on an External row prefetches its playback. */
const EXTERNAL_PREFETCH_HOVER_MS = 250;
/**
 * Experiment: once External results arrive, prefetch the first one's playback
 * as if it were hovered. Turn off to measure without it.
 */
const PREFETCH_FIRST_EXTERNAL_RESULT = true;

type SearchResult = {
	/** Unique across groups; also the option's DOM id suffix. */
	key: string;
	title: string;
	subtitle?: string;
	albumId?: string;
	trackId?: string;
	/** Choosing it opens a page instead of playing; the row says "Open". */
	opensPage?: boolean;
	/** Shown for External Tracks, which have no library page to open. */
	duration?: string;
	/** An External Track's cover, through the Music Server. */
	artworkUrl?: string;
	/** Set on External rows: the track whose playback can be prefetched. */
	externalTrackId?: string;
	action: () => void;
};

/** m:ss, or h:mm:ss from one hour. */
function formatDuration(ms: number): string {
	const total = Math.max(0, Math.round(ms / 1000));
	const hours = Math.floor(total / 3600);
	const minutes = Math.floor((total % 3600) / 60);
	const seconds = (total % 60).toString().padStart(2, "0");
	return hours > 0
		? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds}`
		: `${minutes}:${seconds}`;
}

type SearchGroup = {
	label: string;
	icon: typeof Music2;
	results: SearchResult[];
};

/** The app shell's main element hosts the dialog; it is read on each render. */
function findAppShellMain(): HTMLElement | null {
	return document.querySelector<HTMLElement>("[data-app-shell] main");
}

function subscribeToNothing(): () => void {
	return () => {};
}

/**
 * Searches every library surface at once: Tracks, Albums, Artists, Genres and
 * Playlists. Results are grouped by kind inside the "Library" section;
 * choosing a track plays it, anything else opens its page. When the Music
 * Server offers the External Source, its results follow in a parallel section
 * named after the site (e.g. "YouTube", else "External"), which loads, fails
 * and empties on its own.
 */
export function LibrarySearchDialog({
	open,
	onOpenChange,
	prefetchFirstExternalResult = PREFETCH_FIRST_EXTERNAL_RESULT,
}: {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	/** Defaults to PREFETCH_FIRST_EXTERNAL_RESULT; tests turn it off. */
	prefetchFirstExternalResult?: boolean;
}) {
	const portalContainer = useSyncExternalStore(
		subscribeToNothing,
		findAppShellMain,
		() => null,
	);
	const [query, setQuery] = useState("");
	const [activeKey, setActiveKey] = useState<string | null>(null);
	const [showAll, setShowAll] = useState(false);
	// Reset for the next opening once the dialog closes.
	const [wasOpen, setWasOpen] = useState(open);
	if (open !== wasOpen) {
		setWasOpen(open);
		if (!open) {
			setQuery("");
			setActiveKey(null);
			setShowAll(false);
		}
	}
	const [pendingTracks, setPendingTracks] = useState<Set<string>>(new Set());
	const pendingTrackIds = useRef(new Set<string>());
	const footerRef = useRef<HTMLDivElement>(null);
	const debouncedQuery = useDebouncedValue(query.trim(), SEARCH_DEBOUNCE_MS);
	const hasQuery = /[\p{L}\p{N}]/u.test(query);
	const isDebouncing = query.trim() !== debouncedQuery;
	const canShowResults = open && hasQuery && !isDebouncing;
	const inputRef = useRef<HTMLInputElement>(null);
	const listboxRef = useRef<HTMLDivElement>(null);
	const returnFocus = useReturnFocus();
	const navigate = useNavigate();
	const { playTrack, addToQueue, playExternalTrack } = usePlayback();
	const hasExternalSource = useServerCapability(EXTERNAL_SOURCE_CAPABILITY);
	const externalSourceName = useExternalSourceName();
	const searchLabel =
		hasExternalSource && externalSourceName
			? `Search your library and ${externalSourceName}`
			: "Search your library";
	// The External group shows with the library results; its own query waits
	// for the longer debounce so typing does not start a yt-dlp search per pause.
	const showExternalGroup = canShowResults && hasExternalSource;
	const externalQuery = useDebouncedValue(
		query.trim(),
		EXTERNAL_SEARCH_DEBOUNCE_MS,
	);
	const isDebouncingExternal = query.trim() !== externalQuery;
	const canShowExternal = showExternalGroup && !isDebouncingExternal;
	const queryClient = useQueryClient();
	const prefetchingExternalId = useRef<string | null>(null);
	const hoverPrefetchTimer = useRef<number | undefined>(undefined);
	const listboxId = useId();
	const measureResults = useCallback((content: HTMLDivElement | null) => {
		if (!content) return;
		const card = content.closest<HTMLElement>(".library-search-card");
		const header = inputRef.current?.parentElement;
		if (!card || !header) return;
		const measure = () => {
			const style = getComputedStyle(card);
			const border =
				Number.parseFloat(style.borderTopWidth) +
				Number.parseFloat(style.borderBottomWidth);
			card.style.height = `${Math.min(content.offsetHeight + header.offsetHeight + (footerRef.current?.offsetHeight ?? 0) + border, Number.parseFloat(style.maxHeight))}px`;
		};
		measure();
		// ponytail: measure natural content for WebKit; use CSS auto-size transitions once supported there.
		if (typeof ResizeObserver === "undefined") return;
		const observer = new ResizeObserver(measure);
		observer.observe(content);
		observer.observe(header);
		if (footerRef.current) observer.observe(footerRef.current);
		window.addEventListener("resize", measure);
		return () => {
			observer.disconnect();
			window.removeEventListener("resize", measure);
		};
	}, []);

	useLibrarySearchShortcut(open, onOpenChange);

	const search = useQuery({
		queryKey: [...libraryQueryKeys.search(debouncedQuery), { all: showAll }],
		queryFn: () =>
			showAll
				? apiClient.searchLibrary(debouncedQuery, true)
				: apiClient.searchLibrary(debouncedQuery),
		enabled: canShowResults,
		// Keep the previous result set rendered while a new query loads so rows
		// don't unmount and re-slide with every keystroke.
		placeholderData: keepPreviousData,
		retry: false,
	});

	// No placeholder data: yt-dlp takes seconds, and the previous query's
	// External Tracks would sit under the new query for that long. The signal
	// aborts a superseded search, which ends its yt-dlp process on the server.
	const externalSearch = useQuery({
		queryKey: ["external-search", externalQuery],
		queryFn: async ({ signal }) => {
			const stopSearch = startTiming("external:search");
			const results = await apiClient.searchExternalSource(
				externalQuery,
				undefined,
				signal,
			);
			stopSearch({ results: results.items.length });
			return results;
		},
		enabled: canShowExternal,
		retry: false,
	});

	/** One prefetch at a time: a newer External row replaces the older one. */
	const prefetchExternalPlayback = useCallback(
		(externalTrackId: string) => {
			const previous = prefetchingExternalId.current;
			if (previous === externalTrackId) return;
			if (previous) {
				void queryClient.cancelQueries({
					queryKey: externalPlaybackQueryOptions(previous).queryKey,
					exact: true,
				});
			}
			prefetchingExternalId.current = externalTrackId;
			void queryClient
				.prefetchQuery(externalPlaybackQueryOptions(externalTrackId))
				.finally(() => {
					if (prefetchingExternalId.current === externalTrackId) {
						prefetchingExternalId.current = null;
					}
				});
		},
		[queryClient],
	);
	const cancelHoverPrefetch = useCallback(() => {
		window.clearTimeout(hoverPrefetchTimer.current);
		hoverPrefetchTimer.current = undefined;
	}, []);

	// Closing ends a pending search and hover; an in-flight prefetch is kept,
	// since playing the track may be waiting on it.
	useEffect(() => {
		if (open) return;
		cancelHoverPrefetch();
		void queryClient.cancelQueries({ queryKey: ["external-search"] });
	}, [open, queryClient, cancelHoverPrefetch]);
	useEffect(() => cancelHoverPrefetch, [cancelHoverPrefetch]);

	// The External debounce already waited for typing to stop, so the first
	// result is the likeliest pick: resolve it ahead through the same single
	// prefetch, which a hover or keyboard highlight then replaces.
	const firstExternalTrackId =
		open && canShowExternal && externalSearch.isSuccess
			? externalSearch.data.items[0]?.id
			: undefined;
	useEffect(() => {
		if (!prefetchFirstExternalResult || !firstExternalTrackId) return;
		const cached = queryClient.getQueryState(
			externalPlaybackQueryOptions(firstExternalTrackId).queryKey,
		);
		if (
			cached?.data !== undefined &&
			Date.now() - cached.dataUpdatedAt < EXTERNAL_PLAYBACK_STALE_MS
		) {
			return;
		}
		prefetchExternalPlayback(firstExternalTrackId);
	}, [
		prefetchFirstExternalResult,
		firstExternalTrackId,
		queryClient,
		prefetchExternalPlayback,
	]);

	const close = useCallback(() => onOpenChange(false), [onOpenChange]);

	const groups = useMemo<SearchGroup[]>(() => {
		if (!canShowResults || !search.data) return [];
		const toResult = (
			result: LibrarySearchResult,
			group: string,
		): SearchResult => ({
			key: `${group}:${result.type}:${result.id}`,
			title: result.name,
			albumId: result.type === "album" ? result.id : result.album?.id,
			trackId: result.type === "track" ? result.id : undefined,
			opensPage: result.type !== "track",
			subtitle: [
				getTrackArtistName({ artists: result.artists ?? [], artistName: "" }),
				result.album?.name,
			]
				.filter(Boolean)
				.join(" · "),
			action: () => {
				switch (result.type) {
					case "track":
						void playTrack(result.id);
						break;
					case "album":
						void navigate({
							to: "/library/$albumId",
							params: { albumId: result.id },
						});
						break;
					case "artist":
						void navigate({
							to: "/library/artists/$artistId",
							params: { artistId: result.id },
						});
						break;
					case "genre":
						void navigate({
							to: "/library/genres/$genre",
							params: { genre: result.name },
						});
						break;
					case "playlist":
						void navigate({
							to: "/playlists/$playlistId",
							params: { playlistId: result.id },
						});
						break;
				}
				close();
			},
		});
		const data = search.data;
		const best = data.bestMatch;
		const categories = [
			{ label: "Track", icon: Music2, results: data.tracks },
			{ label: "Album", icon: Disc3, results: data.albums },
			{ label: "Artist", icon: Users, results: data.artists },
			{ label: "Genre", icon: Tags, results: data.genres },
			{ label: "Playlist", icon: ListMusic, results: data.playlists },
		];
		return [
			...(best
				? [
						{
							label: "Best Match",
							icon: Search,
							results: [toResult(best, "best")],
						},
					]
				: []),
			...categories.map((group) => ({
				...group,
				results: group.results
					.slice(0, showAll ? undefined : RESULTS_PER_GROUP)
					.map((result) => toResult(result, group.label.toLowerCase())),
			})),
		].filter((group) => group.results.length > 0);
	}, [canShowResults, search.data, navigate, playTrack, close, showAll]);

	const externalResults = useMemo<SearchResult[]>(() => {
		if (!canShowExternal || !externalSearch.data) return [];
		return externalSearch.data.items.map((track: ExternalTrack) => ({
			key: `external:${track.id}`,
			title: track.title,
			subtitle: track.artistName ?? undefined,
			duration:
				track.durationMs != null ? formatDuration(track.durationMs) : undefined,
			artworkUrl: track.hasArtwork
				? apiClient.getExternalTrackArtworkUrl(track.id)
				: undefined,
			externalTrackId: track.id,
			action: () => {
				// The play path reads this prefetch; a later one must not cancel it.
				if (prefetchingExternalId.current === track.id) {
					prefetchingExternalId.current = null;
				}
				// The dialog closes at once; a failed resolve reports through the
				// app-wide toaster, which outlives the dialog.
				playExternalTrack(track).catch((error: unknown) => {
					toast.error(externalPlaybackFailureMessage(error));
				});
				close();
			},
		}));
	}, [canShowExternal, externalSearch.data, playExternalTrack, close]);

	const libraryResults = useMemo(
		() => groups.flatMap((group) => group.results),
		[groups],
	);
	// Keyboard order follows the screen: library groups, then External.
	const flatResults = useMemo(
		() => [...libraryResults, ...externalResults],
		[libraryResults, externalResults],
	);
	const activeResult =
		flatResults.find((result) => result.key === activeKey) ?? flatResults[0];
	const failed = canShowResults && search.isError;
	const isSearching = hasQuery && (isDebouncing || search.isFetching);
	const showEmpty =
		canShowResults && !isSearching && !failed && libraryResults.length === 0;
	const externalFailed = canShowExternal && externalSearch.isError;
	const isSearchingExternal =
		showExternalGroup && (isDebouncingExternal || externalSearch.isFetching);
	const externalLabel = externalSourceName ?? "External";
	const externalHeader = externalSourceName
		? `External – ${externalSourceName}`
		: "External";
	const externalCount =
		canShowExternal && externalSearch.isSuccess
			? externalResults.length
			: undefined;

	const resultCount =
		canShowResults && search.data
			? (search.data.total ??
				new Set(
					[
						...search.data.tracks,
						...search.data.albums,
						...search.data.artists,
						...search.data.genres,
						...search.data.playlists,
						...(search.data.bestMatch ? [search.data.bestMatch] : []),
					].map((result) => `${result.type}:${result.id}`),
				).size)
			: undefined;

	const queueResult = async (result: SearchResult) => {
		const trackId = result.trackId;
		if (!trackId || pendingTrackIds.current.has(trackId)) return;
		pendingTrackIds.current.add(trackId);
		setPendingTracks(new Set(pendingTrackIds.current));
		try {
			await addToQueue(trackId);
			toast.success(`Added “${result.title}” to queue`);
		} catch (error) {
			console.warn("Failed to add track to queue", { trackId, error });
			toast.error("Failed to add track to queue");
		} finally {
			pendingTrackIds.current.delete(trackId);
			setPendingTracks(new Set(pendingTrackIds.current));
		}
	};

	const scrollActiveResult = useCallback(() => {
		const listbox = listboxRef.current;
		// Only keyboard navigation scrolls: auto-scrolling on every result swap
		// makes rows slide while the card height animates.
		if (!listbox || !activeKey || !activeResult) return;
		const option = document.getElementById(
			optionId(listboxId, activeResult.key),
		);
		if (!option || !listbox.contains(option)) return;
		const viewport = listbox.getBoundingClientRect();
		const row = option.getBoundingClientRect();
		if (row.top < viewport.top) {
			listbox.scrollTop += row.top - viewport.top;
		} else if (row.bottom > viewport.bottom) {
			listbox.scrollTop += row.bottom - viewport.bottom;
		}
	}, [activeKey, activeResult, listboxId]);
	useEffect(scrollActiveResult, [scrollActiveResult]);

	const onInputKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
		if (event.nativeEvent.isComposing || flatResults.length === 0) return;
		if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
			const step = event.key === "ArrowDown" ? 1 : -1;
			const next =
				flatResults[
					(flatResults.indexOf(activeResult) + step + flatResults.length) %
						flatResults.length
				];
			setActiveKey(next.key);
			if (next.externalTrackId) prefetchExternalPlayback(next.externalTrackId);
		} else if (event.key === "Enter") {
			event.preventDefault();
			if (event.repeat) return;
			if (event.shiftKey) void queueResult(activeResult);
			else activeResult.action();
		}
	};

	const renderOption = (result: SearchResult, Icon: typeof Music2) => {
		const isActive = activeResult?.key === result.key;
		return (
			<li
				key={result.key}
				className={cn(
					"flex items-center rounded-md pr-2 transition-colors motion-reduce:transition-none",
					// The keyboard's row looks like the hovered one: the hover step, never the accent.
					isActive && "bg-muted",
				)}
			>
				<button
					type="button"
					id={optionId(listboxId, result.key)}
					role="option"
					aria-selected={isActive}
					tabIndex={-1}
					className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-left outline-none"
					onMouseEnter={() => {
						setActiveKey(result.key);
						cancelHoverPrefetch();
						const { externalTrackId } = result;
						if (!externalTrackId) return;
						hoverPrefetchTimer.current = window.setTimeout(
							() => prefetchExternalPlayback(externalTrackId),
							EXTERNAL_PREFETCH_HOVER_MS,
						);
					}}
					onMouseLeave={cancelHoverPrefetch}
					onMouseDown={(event) => event.preventDefault()}
					onClick={result.action}
				>
					{result.albumId ? (
						<AlbumArt
							coverUrl={apiClient.getAlbumCoverUrl(result.albumId, 128)}
							title={result.title}
							className="size-10 shrink-0 rounded-sm"
						/>
					) : result.artworkUrl ? (
						// The icon stays the placeholder if the cover fails to load.
						<AlbumArt
							coverUrl={result.artworkUrl}
							title={result.title}
							className="size-10 shrink-0 rounded-sm bg-muted/50"
							fallback={<Icon className="size-4" />}
						/>
					) : (
						<span className="flex size-10 shrink-0 items-center justify-center rounded-sm bg-muted/50 text-caption">
							<Icon className="size-4" />
						</span>
					)}
					<span className="min-w-0 flex-1">
						<span
							className={cn(
								"block truncate text-sm",
								isActive && "text-heading",
							)}
						>
							{result.title}
						</span>
						{result.subtitle ? (
							<span className="mt-0.5 block truncate text-caption text-xs">
								{result.subtitle}
							</span>
						) : null}
					</span>
					{result.duration ? (
						<span className="shrink-0 text-caption text-xs tabular-nums">
							{result.duration}
						</span>
					) : null}
					<span
						aria-hidden
						className="flex shrink-0 items-center gap-2 text-caption text-xs"
					>
						{result.opensPage ? (
							<span className="hidden sm:inline">Open</span>
						) : null}
						<CornerDownLeft
							className={cn("size-4", !isActive && "invisible")}
						/>
					</span>
				</button>
				{result.trackId ? (
					<button
						type="button"
						aria-label={`Add ${result.title} to queue`}
						title="Add to queue (Shift+Enter)"
						aria-disabled={pendingTracks.has(result.trackId)}
						onFocus={() => setActiveKey(result.key)}
						onClick={() => {
							void queueResult(result);
						}}
						className="flex size-10 shrink-0 items-center justify-center rounded-md text-caption outline-none hover:bg-foreground/10 hover:text-heading focus-visible:bg-foreground/10 focus-visible:text-heading aria-disabled:opacity-50"
					>
						{pendingTracks.has(result.trackId) ? (
							<LoaderCircle className="size-4 motion-safe:animate-spin" />
						) : (
							<ListPlus className="size-4" />
						)}
					</button>
				) : null}
			</li>
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			{/* Transparent overlay: the page stays as it is behind the search card. */}
			<DialogContent
				container={portalContainer ?? undefined}
				overlayClassName="bg-transparent"
				data-testid="library-search-dialog"
				onTransitionEnd={(event) => {
					if (
						event.target === event.currentTarget &&
						event.propertyName === "height"
					)
						scrollActiveResult();
				}}
				initialFocus={() => {
					returnFocus.capture();
					return inputRef.current;
				}}
				finalFocus={() => returnFocus.restore()}
				className="library-search-card top-[var(--library-search-top)] left-[var(--library-search-center,50%)] flex max-w-(--library-search-width,36rem) translate-y-0 flex-col overflow-hidden p-0"
			>
				<DialogTitle className="sr-only">{searchLabel}</DialogTitle>
				{/* Covers the top bar's search field it opens over; its bottom
				    border lies on the top bar's (library-search-row in styles.css).
				    A container: the card is as wide as that field, not the window. */}
				<div className="library-search-row @container flex shrink-0 items-center gap-3 border-border border-b px-4">
					<Search className="size-5 shrink-0 text-caption" />
					<input
						ref={inputRef}
						type="search"
						role="combobox"
						aria-expanded={flatResults.length > 0}
						aria-controls={listboxId}
						aria-activedescendant={
							activeResult ? optionId(listboxId, activeResult.key) : undefined
						}
						aria-autocomplete="list"
						aria-label={searchLabel}
						maxLength={200}
						placeholder={searchLabel}
						value={query}
						onChange={(event) => {
							setQuery(event.target.value);
							setActiveKey(null);
							setShowAll(false);
						}}
						onKeyDown={onInputKeyDown}
						className="min-w-0 flex-1 self-stretch bg-transparent text-base outline-none placeholder:text-muted-foreground [&::-webkit-search-cancel-button]:appearance-none"
					/>
					{hasExternalSource ? (
						resultCount !== undefined || externalCount !== undefined ? (
							<output className="flex h-8 shrink-0 items-center rounded-sm border border-border px-3 text-caption text-xs tabular-nums">
								{resultCount !== undefined ? (
									<span>{resultCount} in library</span>
								) : null}
								{externalCount !== undefined ? (
									// A narrow card keeps the library count; the External one
									// stays in its section.
									<span
										className={cn(
											resultCount !== undefined && "hidden @lg:inline",
										)}
									>
										{resultCount !== undefined ? " · " : ""}
										{externalSourceName
											? `${externalCount} on ${externalSourceName}`
											: `${externalCount} external`}
									</span>
								) : null}
							</output>
						) : null
					) : resultCount !== undefined ? (
						<output className="flex h-8 shrink-0 items-center rounded-sm border border-border px-3 text-caption text-xs tabular-nums">
							{resultCount} {resultCount === 1 ? "result" : "results"}
						</output>
					) : null}
					<DialogClose
						aria-label="Close search"
						className="flex h-8 shrink-0 items-center rounded-sm border border-border px-3 text-caption text-xs outline-none hover:bg-muted/50 focus-visible:bg-muted/50"
					>
						Esc
					</DialogClose>
				</div>
				<div
					id={listboxId}
					aria-busy={isSearching || isSearchingExternal}
					ref={listboxRef}
					role="listbox"
					aria-label="Search results"
					className="min-h-0 flex-1 overflow-y-auto"
				>
					{/* No query, no body: just the input row and the footer. */}
					<div
						ref={measureResults}
						className={cn(
							"flow-root",
							hasQuery && "flex flex-col gap-6 p-3 sm:p-4",
						)}
					>
						{hasQuery ? (
							// biome-ignore lint/a11y/useSemanticElements: listbox option groups are not form fieldsets.
							<div role="group" aria-label="Library">
								<SectionHeader icon={Library} label="Library" />
								{failed ? (
									<div role="alert" className="px-3 py-2 text-sm">
										<p>
											{libraryResults.length > 0
												? "Search results could not be refreshed."
												: "Search results could not be loaded."}
										</p>
										<button
											type="button"
											className="mt-2 underline"
											disabled={isSearching}
											onClick={() => {
												void search.refetch();
											}}
										>
											Retry
										</button>
									</div>
								) : null}
								{showEmpty ? (
									<p className="px-3 py-2 text-caption text-sm">
										No matches in your library
									</p>
								) : null}
								{isSearching && libraryResults.length === 0 ? (
									<p className="px-3 py-2 text-caption text-sm">Searching…</p>
								) : null}
								{groups.map((group) => (
									// biome-ignore lint/a11y/useSemanticElements: listbox option groups are not form fieldsets.
									<div
										key={group.label}
										role="group"
										aria-label={group.label}
										className="pb-3 last:pb-0"
									>
										<p className="flex items-center gap-2 px-3 py-2 font-medium text-caption text-xs uppercase tracking-wider">
											<group.icon className="size-3.5" />
											{group.label}
										</p>
										<ul className="flex flex-col">
											{group.results.map((result) =>
												renderOption(result, group.icon),
											)}
										</ul>
									</div>
								))}
							</div>
						) : null}
						{showExternalGroup ? (
							// biome-ignore lint/a11y/useSemanticElements: listbox option groups are not form fieldsets.
							<div role="group" aria-label={externalLabel}>
								<SectionHeader icon={Globe} label={externalHeader} />
								{externalResults.length > 0 ? (
									<ul className="flex flex-col">
										{externalResults.map((result) =>
											renderOption(result, Globe),
										)}
									</ul>
								) : null}
								{externalFailed ? (
									<div role="alert" className="px-3 py-2 text-sm">
										<p>{externalLabel} results could not be loaded.</p>
										<button
											type="button"
											className="mt-2 underline"
											disabled={isSearchingExternal}
											onClick={() => {
												void externalSearch.refetch();
											}}
										>
											Retry
										</button>
									</div>
								) : isSearchingExternal && externalResults.length === 0 ? (
									<p className="px-3 py-2 text-caption text-sm">Searching…</p>
								) : externalSearch.isSuccess && externalResults.length === 0 ? (
									<p className="px-3 py-2 text-caption text-sm">
										{externalSourceName
											? `No results on ${externalSourceName}`
											: "No External results"}
									</p>
								) : null}
							</div>
						) : null}
					</div>
				</div>
				<div
					ref={footerRef}
					className="flex shrink-0 flex-wrap items-center justify-between gap-x-4 gap-y-3 border-border border-t px-4 py-3 text-caption text-xs sm:px-6"
				>
					<div className="flex flex-wrap items-center gap-3">
						<span className="inline-flex items-center gap-2">
							<kbd className="rounded-sm border border-border p-1">
								<ArrowUp className="size-3" />
							</kbd>
							<kbd className="rounded-sm border border-border p-1">
								<ArrowDown className="size-3" />
							</kbd>{" "}
							Navigate
						</span>
						<span className="inline-flex items-center gap-2">
							<kbd className="rounded-sm border border-border p-1">
								<CornerDownLeft className="size-3" />
							</kbd>{" "}
							Play / open
						</span>
						<span className="inline-flex items-center gap-2">
							<kbd className="rounded-sm border border-border px-2 py-1">
								Shift ↵
							</kbd>{" "}
							Add to queue
						</span>
					</div>
					{hasQuery ? (
						<button
							type="button"
							disabled={isSearching || failed || !resultCount}
							onClick={() => {
								setShowAll(!showAll);
								inputRef.current?.focus();
							}}
							className="inline-flex min-w-0 max-w-full items-center gap-2 rounded-sm px-1 py-1 text-foreground outline-none hover:text-heading hover:underline focus-visible:underline disabled:opacity-50"
						>
							<span className="truncate">
								{showAll
									? "Show top results"
									: `See all results for “${query.trim()}”`}
							</span>
							<CornerDownLeft className="size-4 shrink-0" />
						</button>
					) : null}
				</div>
			</DialogContent>
		</Dialog>
	);
}

/**
 * A source section's header: Library or the External Source. It keeps its
 * case, unlike the uppercase kind headers inside the Library section, so the
 * two levels read apart.
 */
function SectionHeader({
	icon: Icon,
	label,
}: {
	icon: typeof Music2;
	label: string;
}) {
	return (
		<p className="flex items-center gap-2 px-3 py-2 font-medium text-caption text-xs">
			<Icon className="size-3.5" />
			{label}
		</p>
	);
}

function optionId(listboxId: string, key: string): string {
	return `${listboxId}-${encodeURIComponent(key)}`;
}

function isTypingTarget(target: EventTarget | null): boolean {
	if (!(target instanceof HTMLElement)) return false;
	return Boolean(target.closest("input, textarea, select, [contenteditable]"));
}

/** Ctrl/⌘+K toggles the dialog; "/" opens it unless a field is focused. */
function useLibrarySearchShortcut(
	open: boolean,
	onOpenChange: (open: boolean) => void,
) {
	useEffect(() => {
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.defaultPrevented || event.repeat) return;
			const isCommandK =
				(event.ctrlKey || event.metaKey) &&
				!event.altKey &&
				!event.shiftKey &&
				event.key.toLowerCase() === "k";
			if (isCommandK) {
				event.preventDefault();
				onOpenChange(!open);
				return;
			}
			if (
				event.key === "/" &&
				!event.ctrlKey &&
				!event.metaKey &&
				!event.altKey &&
				!open &&
				!isTypingTarget(event.target)
			) {
				event.preventDefault();
				onOpenChange(true);
			}
		};
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, [open, onOpenChange]);
}
