import type {
	AcquisitionScope,
	AcquisitionSource,
	AcquisitionVersion,
	ExternalTrack,
	SoulseekAcquisitionCreate,
} from "@repo/api-client";
import { toast } from "@repo/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, LoaderCircle, X } from "lucide-react";
import { useContext, useId, useState } from "react";
import { ImportSessionContext } from "#/components/import-session-context";
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";
import {
	Dialog,
	DialogClose,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "#/components/ui/dialog";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuGroup,
	DropdownMenuLabel,
	DropdownMenuRadioGroup,
	DropdownMenuRadioItem,
	DropdownMenuTrigger,
} from "#/components/ui/dropdown-menu";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { RadioGroup, RadioGroupItem } from "#/components/ui/radio-group";
import { apiClient } from "#/lib/api";
import {
	buildSoulseekQuery,
	formatAcquisitionQuality,
	formatQualityChoice,
	formatQualityChoiceValue,
	formatSize,
	formatSpeed,
	parseQualityChoice,
	QUALITY_GROUPS,
	type QualityChoice,
	qualityChoiceOf,
	qualityMatches,
	soulseekErrorMessage,
	soulseekQueryKeys,
	soulseekQueryWithoutArtist,
} from "#/lib/soulseek";
import { cn } from "#/lib/utils";

const SEARCH_POLL_INTERVAL_MS = 1000;

const scopeOptions: Array<{ value: AcquisitionScope; label: string }> = [
	{ value: "album", label: "Album" },
	{ value: "track", label: "Track" },
];

/** The last quality chosen for a scope, kept on this device. */
const qualityStorageKey = (scope: AcquisitionScope) =>
	`earthly-audio-soulseek-quality-${scope}`;

function loadQuality(scope: AcquisitionScope): QualityChoice | null {
	try {
		return parseQualityChoice(
			window.localStorage.getItem(qualityStorageKey(scope)),
		);
	} catch {
		return null;
	}
}

function storeQuality(scope: AcquisitionScope, choice: QualityChoice) {
	try {
		window.localStorage.setItem(qualityStorageKey(scope), choice.key);
	} catch {
		// Remembering the quality is a convenience; the choice still applies.
	}
}

/** A source with the version it belongs to, which installing it names. */
interface MatchingSource {
	version: AcquisitionVersion;
	source: AcquisitionSource;
}

/**
 * Searches Soulseek for an External Track in the quality the listener
 * chooses from a fixed list, and starts a Soulseek Acquisition from the
 * sources that offer it. Only that quality is shown; none is highlighted or
 * called best. Changing the quality filters the same search again. Closing
 * the dialog never cancels an acquisition: it continues on the Import Music
 * dialog.
 */
export function SoulseekInstallDialog({
	track,
	open,
	onOpenChange,
	finalFocus,
}: {
	track: ExternalTrack;
	open: boolean;
	onOpenChange: (open: boolean) => void;
	finalFocus?: () => HTMLElement | true;
}) {
	const queryClient = useQueryClient();
	const importSession = useContext(ImportSessionContext);
	const [scope, setScope] = useState<AcquisitionScope>("album");
	const [query, setQuery] = useState(() => buildSoulseekQuery(track, "album"));
	// The query follows the scope until the listener edits it.
	const [queryEdited, setQueryEdited] = useState(false);
	const [quality, setQuality] = useState<QualityChoice | null>(() =>
		loadQuality("album"),
	);
	const queryId = useId();

	const startSearch = useMutation({
		mutationFn: (request: { query: string; scope: AcquisitionScope }) =>
			apiClient.startSoulseekSearch(request.query, request.scope),
	});
	const searchId = startSearch.data?.searchId;
	const search = useQuery({
		queryKey: soulseekQueryKeys.search(searchId ?? ""),
		queryFn: () => apiClient.getSoulseekSearch(searchId ?? ""),
		enabled: Boolean(searchId),
		refetchInterval: (current) =>
			current.state.data?.state === "complete"
				? false
				: SEARCH_POLL_INTERVAL_MS,
	});
	const startAcquisition = useMutation({
		mutationFn: (request: SoulseekAcquisitionCreate) =>
			apiClient.startSoulseekAcquisition(request),
		onSuccess: (acquisition) => {
			void queryClient.invalidateQueries({
				queryKey: soulseekQueryKeys.acquisitions,
			});
			toast.success(
				`Installing “${acquisition.query}”. Follow it in Import Music.`,
			);
			onOpenChange(false);
			importSession?.open();
		},
	});

	const trimmedQuery = query.trim();
	const versions = search.data?.versions ?? [];
	const matching = quality
		? versions.filter((version) => qualityMatches(version.quality, quality))
		: [];
	const isSearching =
		startSearch.isPending ||
		(Boolean(searchId) && search.data?.state !== "complete" && !search.isError);
	const searchError = startSearch.error ?? search.error;
	const searched = startSearch.variables;
	const isComplete =
		!isSearching && search.data?.state === "complete" && !searchError;
	const foundNothing = isComplete && versions.length === 0;
	const queryWithoutArtist =
		foundNothing && searched
			? soulseekQueryWithoutArtist(track, searched.scope, searched.query)
			: "";
	const chooseQuality = (choice: QualityChoice) => {
		setQuality(choice);
		storeQuality(scope, choice);
	};
	const runSearch = (request: { query: string; scope: AcquisitionScope }) => {
		if (!request.query || !quality || startSearch.isPending) return;
		startSearch.mutate(request);
	};
	const install = (version: AcquisitionVersion, source?: AcquisitionSource) => {
		if (!searchId || startAcquisition.isPending) return;
		startAcquisition.mutate({
			searchId,
			versionKey: version.versionKey,
			...(source ? { sourceId: source.sourceId } : {}),
		});
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent
				finalFocus={finalFocus}
				className="flex max-h-dialog-height max-w-2xl flex-col overflow-hidden p-0"
			>
				<div className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-4 py-4 sm:px-6">
					<div className="min-w-0">
						<DialogTitle>Install</DialogTitle>
						<DialogDescription className="mt-1">
							Find “{track.title}” on Soulseek in the quality you choose.
						</DialogDescription>
					</div>
					<DialogClose
						render={<Button type="button" variant="ghost" size="icon" />}
					>
						<X className="size-4" />
						<span className="sr-only">Close Install</span>
					</DialogClose>
				</div>
				<form
					className="grid shrink-0 gap-4 border-border border-b px-4 py-4 sm:px-6"
					onSubmit={(event) => {
						event.preventDefault();
						runSearch({ query: trimmedQuery, scope });
					}}
				>
					<fieldset className="grid gap-2">
						<legend className="mb-2 font-medium text-sm">
							What to install
						</legend>
						<RadioGroup
							aria-label="What to install"
							value={scope}
							onValueChange={(value) => {
								const next = value as AcquisitionScope;
								setScope(next);
								if (!queryEdited) setQuery(buildSoulseekQuery(track, next));
								const remembered = loadQuality(next);
								if (remembered) setQuality(remembered);
							}}
						>
							{scopeOptions.map((option) => (
								<Label key={option.value} className="font-normal">
									<RadioGroupItem value={option.value} />
									{option.label}
								</Label>
							))}
						</RadioGroup>
					</fieldset>
					<div className="grid gap-2">
						<Label htmlFor={queryId}>Soulseek query</Label>
						<div className="flex gap-2">
							<Input
								id={queryId}
								value={query}
								maxLength={200}
								onChange={(event) => {
									setQuery(event.target.value);
									setQueryEdited(true);
								}}
							/>
							<QualityPicker value={quality} onChange={chooseQuality} />
							<Button
								type="submit"
								disabled={!trimmedQuery || !quality || startSearch.isPending}
							>
								Search Soulseek
							</Button>
						</div>
					</div>
				</form>
				<section
					aria-label="Sources"
					aria-busy={isSearching}
					className="min-h-0 flex-1 overflow-y-auto overscroll-contain scrollbar-hidden"
				>
					<SearchStatus
						isSearching={isSearching}
						hasSearched={Boolean(searchId)}
						quality={quality}
						matchingSourceCount={matching.reduce(
							(count, version) => count + version.sources.length,
							0,
						)}
						versionCount={versions.length}
						error={searchError}
					/>
					{foundNothing ? (
						<div className="flex flex-wrap items-center gap-3 px-4 pb-3 sm:px-6">
							<p className="text-caption text-sm">
								Soulseek filters some artist names, so a query naming one finds
								nothing.
							</p>
							{queryWithoutArtist && searched ? (
								<Button
									type="button"
									variant="outline"
									size="sm"
									disabled={startSearch.isPending}
									onClick={() => {
										setQuery(queryWithoutArtist);
										setQueryEdited(true);
										runSearch({
											query: queryWithoutArtist,
											scope: searched.scope,
										});
									}}
								>
									Search without the artist
								</Button>
							) : null}
						</div>
					) : null}
					{isComplete && quality && matching.length === 0 && versions.length ? (
						<FoundQualities versions={versions} onChoose={chooseQuality} />
					) : null}
					{startAcquisition.error ? (
						<p
							role="alert"
							className="mx-4 my-3 rounded-sm bg-destructive/10 px-3 py-2 text-destructive text-sm sm:mx-6"
						>
							{soulseekErrorMessage(startAcquisition.error)}
						</p>
					) : null}
					{quality && matching.length > 0 ? (
						<MatchingSources
							quality={quality}
							versions={matching}
							scope={search.data?.scope ?? scope}
							isBusy={startAcquisition.isPending}
							onInstall={install}
						/>
					) : null}
				</section>
			</DialogContent>
		</Dialog>
	);
}

/** The Quality button and its fixed list, one value chosen at a time. */
function QualityPicker({
	value,
	onChange,
}: {
	value: QualityChoice | null;
	onChange: (choice: QualityChoice) => void;
}) {
	const label = value ? formatQualityChoice(value) : null;
	return (
		<DropdownMenu>
			<DropdownMenuTrigger
				render={
					<Button
						type="button"
						variant="outline"
						aria-label={label ? `Quality, ${label}` : "Quality"}
					/>
				}
			>
				{label ?? "Quality"}
				<ChevronDown aria-hidden="true" />
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end" className="w-80">
				<div className="grid gap-2 pb-2">
					{QUALITY_GROUPS.map((group) => (
						<DropdownMenuGroup key={group.format}>
							<DropdownMenuLabel>
								{group.label} · {group.unit}
							</DropdownMenuLabel>
							<DropdownMenuRadioGroup
								value={value?.format === group.format ? value.key : ""}
								onValueChange={(key) => {
									const choice = parseQualityChoice(String(key));
									if (choice) onChange(choice);
								}}
							>
								<div className="flex flex-wrap gap-1 px-2">
									{group.choices.map((choice) => (
										<DropdownMenuRadioItem
											key={choice.key}
											variant="chip"
											value={choice.key}
											closeOnClick
											aria-label={formatQualityChoice(choice)}
										>
											{formatQualityChoiceValue(choice)}
										</DropdownMenuRadioItem>
									))}
								</div>
							</DropdownMenuRadioGroup>
						</DropdownMenuGroup>
					))}
				</div>
			</DropdownMenuContent>
		</DropdownMenu>
	);
}

function SearchStatus({
	isSearching,
	hasSearched,
	quality,
	matchingSourceCount,
	versionCount,
	error,
}: {
	isSearching: boolean;
	hasSearched: boolean;
	quality: QualityChoice | null;
	matchingSourceCount: number;
	versionCount: number;
	error: Error | null;
}) {
	if (error)
		return (
			<p
				role="alert"
				className="mx-4 my-3 rounded-sm bg-destructive/10 px-3 py-2 text-destructive text-sm sm:mx-6"
			>
				{soulseekErrorMessage(error)}
			</p>
		);
	if (!hasSearched && !isSearching)
		return (
			<p className="px-4 py-3 text-caption text-sm sm:px-6">
				{quality
					? "Edit the query if needed, then search."
					: "Choose a quality, then search."}
			</p>
		);
	const label = quality ? formatQualityChoice(quality) : "";
	const sources = `${matchingSourceCount} ${matchingSourceCount === 1 ? "source" : "sources"}`;
	return (
		<p
			aria-live="polite"
			className="flex items-center gap-2 px-4 py-3 text-caption text-sm sm:px-6"
		>
			{isSearching ? (
				<LoaderCircle
					aria-hidden="true"
					className="size-3.5 motion-safe:animate-spin"
				/>
			) : null}
			{isSearching
				? matchingSourceCount > 0
					? `Searching Soulseek… ${sources} for ${label} so far`
					: "Searching Soulseek…"
				: matchingSourceCount > 0
					? `${sources} for ${label}`
					: versionCount > 0
						? `No source offers ${label}.`
						: "Nothing found. Edit the query and search again."}
		</p>
	);
}

/**
 * The qualities a search found when none is the chosen one: listed ones
 * switch the quality without searching again; others are named only.
 */
function FoundQualities({
	versions,
	onChoose,
}: {
	versions: AcquisitionVersion[];
	onChoose: (choice: QualityChoice) => void;
}) {
	const listed = new Map<string, QualityChoice>();
	const unlisted = new Set<string>();
	for (const version of versions) {
		const choice = qualityChoiceOf(version.quality);
		if (choice) listed.set(choice.key, choice);
		else unlisted.add(formatAcquisitionQuality(version.quality));
	}
	return (
		<div className="grid gap-2 px-4 pb-3 sm:px-6">
			<p className="text-caption text-sm">Found instead:</p>
			<ul aria-label="Found qualities" className="flex flex-wrap gap-2">
				{[...listed.values()].map((choice) => (
					<li key={choice.key}>
						<Button
							type="button"
							variant="outline"
							size="sm"
							onClick={() => onChoose(choice)}
						>
							{formatQualityChoice(choice)}
						</Button>
					</li>
				))}
				{[...unlisted].map((label) => (
					<li key={label} className="flex items-center">
						<Badge variant="outline">{label}</Badge>
					</li>
				))}
			</ul>
		</div>
	);
}

/**
 * The sources of the chosen quality as one list. Install takes the first
 * matching version and lets the Music Server pick its source.
 */
function MatchingSources({
	quality,
	versions,
	scope,
	isBusy,
	onInstall,
}: {
	quality: QualityChoice;
	versions: AcquisitionVersion[];
	scope: AcquisitionScope;
	isBusy: boolean;
	onInstall: (version: AcquisitionVersion, source?: AcquisitionSource) => void;
}) {
	const label = formatQualityChoice(quality);
	const [primary] = versions;
	const sources: MatchingSource[] = versions.flatMap((version) =>
		version.sources.map((source) => ({ version, source })),
	);
	const complete = sources.filter(({ source }) => source.complete).length;
	if (!primary) return null;
	return (
		<div className="px-4 pb-3 sm:px-6">
			<div className="flex items-center gap-3 border-border/70 border-b pb-3">
				<div className="min-w-0 flex-1">
					<p className="truncate font-medium text-heading text-sm">{label}</p>
					<p className="text-caption text-xs tabular-nums">
						{primary.fileCount} {primary.fileCount === 1 ? "file" : "files"} ·{" "}
						{formatSize(primary.totalBytes)} · {sources.length}{" "}
						{sources.length === 1 ? "source" : "sources"}
						{scope === "album" && complete < sources.length
							? ` (${complete} complete)`
							: ""}
					</p>
				</div>
				<Button
					type="button"
					size="sm"
					aria-label={`Install ${label}`}
					disabled={isBusy}
					onClick={() => onInstall(primary)}
				>
					Install
				</Button>
			</div>
			<ul aria-label={`Sources for ${label}`}>
				{sources.map(({ version, source }) => (
					<SourceRow
						key={`${version.versionKey}/${source.sourceId}`}
						source={source}
						versionFileCount={version.fileCount}
						isBusy={isBusy}
						onInstall={() => onInstall(version, source)}
					/>
				))}
			</ul>
		</div>
	);
}

function SourceRow({
	source,
	versionFileCount,
	isBusy,
	onInstall,
}: {
	source: AcquisitionSource;
	versionFileCount: number;
	isBusy: boolean;
	onInstall: () => void;
}) {
	return (
		<li className="flex items-center gap-3 border-border/70 border-b py-2 last:border-b-0">
			<div className="min-w-0 flex-1">
				<p className="truncate text-sm" title={source.username}>
					{source.username}
				</p>
				<p className="truncate text-caption text-xs" title={source.folder}>
					{source.folder}
				</p>
			</div>
			<p className="shrink-0 text-right text-caption text-xs tabular-nums">
				<span className={cn(!source.complete && "text-foreground")}>
					{source.complete
						? `${source.fileCount} ${source.fileCount === 1 ? "file" : "files"}`
						: `${source.fileCount}/${versionFileCount} files`}
				</span>
				<span className="block">
					{source.hasFreeUploadSlot ? "Free slot" : "No free slot"} · queue{" "}
					{source.queueLength} · {formatSpeed(source.uploadSpeedBytesPerSecond)}
				</span>
			</p>
			<Button
				type="button"
				variant="ghost"
				size="sm"
				aria-label={`Install from ${source.username}`}
				disabled={isBusy}
				onClick={onInstall}
			>
				Install
			</Button>
		</li>
	);
}
