import type { LibrarySearchResult } from "@repo/api-client";
import { AlbumArt, toast, usePlayback } from "@repo/ui";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
	ArrowDown,
	ArrowUp,
	CornerDownLeft,
	Disc3,
	ListMusic,
	ListPlus,
	LoaderCircle,
	Music2,
	Search,
	Tags,
	Users,
} from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import {
	useCallback,
	useEffect,
	useId,
	useMemo,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import { useDebouncedValue } from "#/hooks/use-debounced-value";
import { useReturnFocus } from "#/hooks/use-return-focus";
import { apiClient } from "#/lib/api";
import { getTrackArtistName } from "#/lib/library-display";
import { libraryQueryKeys } from "#/lib/library-query-keys";
import { cn } from "#/lib/utils";

const RESULTS_PER_GROUP = 5;
const SEARCH_DEBOUNCE_MS = 200;

type SearchResult = {
	/** Unique across groups; also the option's DOM id suffix. */
	key: string;
	title: string;
	subtitle?: string;
	albumId?: string;
	trackId?: string;
	action: () => void;
};

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
 * Playlists. Results are grouped under "From Your Library"; choosing a track
 * plays it, anything else opens its page.
 */
export function LibrarySearchDialog({
	open,
	onOpenChange,
}: {
	open: boolean;
	onOpenChange: (open: boolean) => void;
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
	const { playTrack, addToQueue } = usePlayback();
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
							to: "/library/tracks",
							search: { artistId: result.id },
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

	const flatResults = useMemo(
		() => groups.flatMap((group) => group.results),
		[groups],
	);
	const activeResult =
		flatResults.find((result) => result.key === activeKey) ?? flatResults[0];
	const failed = canShowResults && search.isError;
	const isSearching = hasQuery && (isDebouncing || search.isFetching);
	const showEmpty =
		canShowResults && !isSearching && !failed && flatResults.length === 0;

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
		if (event.key === "ArrowDown") {
			event.preventDefault();
			setActiveKey(
				flatResults[
					(flatResults.indexOf(activeResult) + 1) % flatResults.length
				].key,
			);
		} else if (event.key === "ArrowUp") {
			event.preventDefault();
			setActiveKey(
				flatResults[
					(flatResults.indexOf(activeResult) - 1 + flatResults.length) %
						flatResults.length
				].key,
			);
		} else if (event.key === "Enter") {
			event.preventDefault();
			if (event.repeat) return;
			if (event.shiftKey) void queueResult(activeResult);
			else activeResult.action();
		}
	};

	return (
		<DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
			<DialogPrimitive.Portal container={portalContainer}>
				{/* Transparent: the page stays as it is behind the search card. */}
				<DialogPrimitive.Overlay className="fixed inset-0 z-50" />
				<DialogPrimitive.Content
					data-testid="library-search-dialog"
					onTransitionEnd={(event) => {
						if (
							event.target === event.currentTarget &&
							event.propertyName === "height"
						)
							scrollActiveResult();
					}}
					onOpenAutoFocus={(event) => {
						returnFocus.capture();
						event.preventDefault();
						inputRef.current?.focus();
					}}
					onCloseAutoFocus={returnFocus.restore}
					aria-describedby={undefined}
					className="library-search-card fixed z-50 flex -translate-x-1/2 flex-col overflow-hidden rounded-lg border border-border bg-popover text-popover-foreground shadow-xl outline-none"
				>
					<DialogPrimitive.Title className="sr-only">
						Search your library
					</DialogPrimitive.Title>
					<div className="flex shrink-0 items-center gap-3 border-border border-b px-4 sm:gap-4 sm:px-6">
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
							aria-label="Search your library"
							maxLength={200}
							placeholder="Search tracks, albums, artists, genres, playlists"
							value={query}
							onChange={(event) => {
								setQuery(event.target.value);
								setActiveKey(null);
								setShowAll(false);
							}}
							onKeyDown={onInputKeyDown}
							className="h-16 min-w-0 flex-1 bg-transparent text-xl outline-none placeholder:text-muted-foreground sm:h-20 [&::-webkit-search-cancel-button]:appearance-none"
						/>
						{resultCount !== undefined ? (
							<output className="shrink-0 rounded-sm border border-border px-3 py-2 text-caption text-xs">
								{resultCount} {resultCount === 1 ? "result" : "results"}
							</output>
						) : null}
						<DialogPrimitive.Close
							aria-label="Close search"
							className="shrink-0 rounded-sm border border-border px-3 py-2 text-caption text-xs outline-none hover:bg-muted/50 focus-visible:bg-muted/50"
						>
							Esc
						</DialogPrimitive.Close>
					</div>
					<div
						id={listboxId}
						aria-busy={isSearching}
						ref={listboxRef}
						role="listbox"
						aria-label="Search results"
						className="min-h-0 flex-1 overflow-y-auto"
					>
						<div ref={measureResults} className="flow-root p-3 sm:p-4">
							{!hasQuery ? (
								<p className="px-3 py-6 text-center text-caption text-sm">
									Type to search your library.
								</p>
							) : null}
							{failed ? (
								<div role="alert" className="px-3 py-3 text-sm">
									<p>
										{flatResults.length > 0
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
								<p className="px-3 py-6 text-center text-caption text-sm">
									Nothing in your library matches “{debouncedQuery}”.
								</p>
							) : null}
							{isSearching && flatResults.length === 0 ? (
								<p className="px-3 py-6 text-center text-caption text-sm">
									Searching…
								</p>
							) : null}
							{groups.length > 0 ? (
								<section aria-label="From Your Library">
									<p className="sr-only">From Your Library</p>
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
												{group.results.map((result) => {
													const isActive = activeResult?.key === result.key;
													return (
														<li
															key={result.key}
															className={cn(
																"flex items-center rounded-md border pr-2 transition-colors motion-reduce:transition-none",
																isActive
																	? "border-shell-active-foreground bg-shell-active"
																	: "border-transparent hover:bg-muted/50",
															)}
														>
															<button
																type="button"
																id={optionId(listboxId, result.key)}
																role="option"
																aria-selected={isActive}
																tabIndex={-1}
																className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-md p-3 text-left outline-none sm:gap-4"
																onMouseEnter={() => setActiveKey(result.key)}
																onMouseDown={(event) => event.preventDefault()}
																onClick={result.action}
															>
																{result.albumId ? (
																	<AlbumArt
																		coverUrl={apiClient.getAlbumCoverUrl(
																			result.albumId,
																			128,
																		)}
																		title={result.title}
																		className="size-11 shrink-0 rounded-sm sm:size-12"
																	/>
																) : (
																	<span className="flex size-11 shrink-0 items-center justify-center rounded-sm bg-muted/50 text-caption sm:size-12">
																		<group.icon className="size-5" />
																	</span>
																)}
																<span className="min-w-0 flex-1">
																	<span
																		className={cn(
																			"block truncate text-base",
																			isActive &&
																				"text-shell-active-foreground",
																		)}
																	>
																		{result.title}
																	</span>
																	{result.subtitle ? (
																		<span className="mt-0.5 block truncate text-caption text-sm">
																			{result.subtitle}
																		</span>
																	) : null}
																</span>
																<span
																	aria-hidden
																	className="flex shrink-0 items-center gap-2 text-caption text-xs"
																>
																	{!result.trackId ? (
																		<span className="hidden sm:inline">
																			Open
																		</span>
																	) : null}
																	<CornerDownLeft
																		className={cn(
																			"size-4 text-shell-active-foreground",
																			!isActive && "invisible",
																		)}
																	/>
																</span>
															</button>
															{result.trackId ? (
																<button
																	type="button"
																	aria-label={`Add ${result.title} to queue`}
																	title="Add to queue (Shift+Enter)"
																	aria-disabled={pendingTracks.has(
																		result.trackId,
																	)}
																	onFocus={() => setActiveKey(result.key)}
																	onClick={() => {
																		void queueResult(result);
																	}}
																	className="flex size-11 shrink-0 items-center justify-center rounded-md text-caption outline-none hover:bg-foreground/10 hover:text-shell-active-foreground focus-visible:bg-foreground/10 focus-visible:text-shell-active-foreground aria-disabled:opacity-50"
																>
																	{pendingTracks.has(result.trackId) ? (
																		<LoaderCircle className="size-5 motion-safe:animate-spin" />
																	) : (
																		<ListPlus className="size-5" />
																	)}
																</button>
															) : null}
														</li>
													);
												})}
											</ul>
										</div>
									))}
								</section>
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
								className="inline-flex min-w-0 max-w-full items-center gap-2 rounded-sm px-1 py-1 text-shell-active-foreground outline-none hover:underline focus-visible:underline disabled:opacity-50"
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
				</DialogPrimitive.Content>
			</DialogPrimitive.Portal>
		</DialogPrimitive.Root>
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
