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
import { Button } from "#/components/ui/button";
import {
	Dialog,
	DialogClose,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "#/components/ui/dialog";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { RadioGroup, RadioGroupItem } from "#/components/ui/radio-group";
import { apiClient } from "#/lib/api";
import {
	buildSoulseekQuery,
	formatAcquisitionQuality,
	formatSize,
	formatSpeed,
	soulseekErrorMessage,
	soulseekQueryKeys,
} from "#/lib/soulseek";
import { cn } from "#/lib/utils";

const SEARCH_POLL_INTERVAL_MS = 1000;

const scopeOptions: Array<{ value: AcquisitionScope; label: string }> = [
	{ value: "album", label: "Album" },
	{ value: "track", label: "Track" },
];

/**
 * Searches Soulseek for an External Track and starts a Soulseek Acquisition
 * of the Acquisition Version the listener chooses. Versions keep the Music
 * Server's display order; none is highlighted or called best, so the choice
 * of quality stays the listener's. Closing the dialog never cancels an
 * acquisition: it continues on the Import Music dialog.
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
	const [query, setQuery] = useState(() => buildSoulseekQuery(track));
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
	const isSearching =
		startSearch.isPending ||
		(Boolean(searchId) && search.data?.state !== "complete" && !search.isError);
	const searchError = startSearch.error ?? search.error;
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
							Find “{track.title}” on Soulseek and choose a version to import.
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
						if (!trimmedQuery || startSearch.isPending) return;
						startSearch.mutate({ query: trimmedQuery, scope });
					}}
				>
					<fieldset className="grid gap-2">
						<legend className="mb-2 font-medium text-sm">
							What to install
						</legend>
						<RadioGroup
							aria-label="What to install"
							value={scope}
							onValueChange={(value) => setScope(value as AcquisitionScope)}
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
								onChange={(event) => setQuery(event.target.value)}
							/>
							<Button
								type="submit"
								disabled={!trimmedQuery || startSearch.isPending}
							>
								Search Soulseek
							</Button>
						</div>
					</div>
				</form>
				<section
					aria-label="Acquisition Versions"
					aria-busy={isSearching}
					className="min-h-0 flex-1 overflow-y-auto overscroll-contain scrollbar-hidden"
				>
					<SearchStatus
						isSearching={isSearching}
						hasSearched={Boolean(searchId)}
						versionCount={versions.length}
						error={searchError}
					/>
					{startAcquisition.error ? (
						<p
							role="alert"
							className="mx-4 my-3 rounded-sm bg-destructive/10 px-3 py-2 text-destructive text-sm sm:mx-6"
						>
							{soulseekErrorMessage(startAcquisition.error)}
						</p>
					) : null}
					{versions.length > 0 ? (
						<ul>
							{versions.map((version) => (
								<VersionRow
									key={version.versionKey}
									version={version}
									scope={search.data?.scope ?? scope}
									isBusy={startAcquisition.isPending}
									onInstall={install}
								/>
							))}
						</ul>
					) : null}
				</section>
			</DialogContent>
		</Dialog>
	);
}

function SearchStatus({
	isSearching,
	hasSearched,
	versionCount,
	error,
}: {
	isSearching: boolean;
	hasSearched: boolean;
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
				Edit the query if needed, then search.
			</p>
		);
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
				? versionCount > 0
					? `Searching Soulseek… ${versionCount} ${versionCount === 1 ? "version" : "versions"} so far`
					: "Searching Soulseek…"
				: versionCount > 0
					? `${versionCount} ${versionCount === 1 ? "version" : "versions"} found`
					: "No versions found. Edit the query and search again."}
		</p>
	);
}

function VersionRow({
	version,
	scope,
	isBusy,
	onInstall,
}: {
	version: AcquisitionVersion;
	scope: AcquisitionScope;
	isBusy: boolean;
	onInstall: (version: AcquisitionVersion, source?: AcquisitionSource) => void;
}) {
	const [isExpanded, setIsExpanded] = useState(false);
	const sourcesId = useId();
	const quality = formatAcquisitionQuality(version.quality);
	const sources = `${version.sourceCount} ${version.sourceCount === 1 ? "source" : "sources"}`;
	const incomplete = version.sourceCount - version.completeSourceCount;
	return (
		<li
			aria-label={quality}
			className="border-border/70 border-b px-4 py-3 last:border-b-0 sm:px-6"
		>
			<div className="flex items-center gap-3">
				<div className="min-w-0 flex-1">
					<p
						className="truncate font-medium text-heading text-sm"
						title={quality}
					>
						{quality}
					</p>
					<p className="text-caption text-xs tabular-nums">
						{version.fileCount} {version.fileCount === 1 ? "file" : "files"} ·{" "}
						{formatSize(version.totalBytes)} · {sources}
						{scope === "album" && incomplete > 0
							? ` (${version.completeSourceCount} complete)`
							: ""}
					</p>
				</div>
				<Button
					type="button"
					variant="muted"
					size="icon-sm"
					aria-label={`Sources for ${quality}`}
					title="Show sources"
					aria-expanded={isExpanded}
					aria-controls={isExpanded ? sourcesId : undefined}
					onClick={() => setIsExpanded(!isExpanded)}
				>
					<ChevronDown
						aria-hidden="true"
						className={cn(
							"size-4 transition-transform motion-reduce:transition-none",
							isExpanded && "rotate-180",
						)}
					/>
				</Button>
				<Button
					type="button"
					variant="outline"
					size="sm"
					aria-label={`Install ${quality}`}
					disabled={isBusy}
					onClick={() => onInstall(version)}
				>
					Install
				</Button>
			</div>
			{isExpanded ? (
				<ul
					id={sourcesId}
					aria-label={`Sources for ${quality}`}
					className="mt-2 border-border/70 border-t"
				>
					{version.sources.map((source) => (
						<SourceRow
							key={source.sourceId}
							source={source}
							versionFileCount={version.fileCount}
							isBusy={isBusy}
							onInstall={() => onInstall(version, source)}
						/>
					))}
				</ul>
			) : null}
		</li>
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
