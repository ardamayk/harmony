import type {
	ManagedImportAlbumDecision,
	ManagedImportAlbumPreview,
} from "@repo/api-client";
import { useId, useState } from "react";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { RadioGroup, RadioGroupItem } from "#/components/ui/radio-group";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "#/components/ui/select";
import { apiClient } from "#/lib/api";
import { ImportCoverPicker } from "./-import-cover-picker";
import { defaultAlbumDecision, importPositionKey } from "./-import-review";
import type { ImportFileEntry } from "./-managed-import-workflow";

type Props = {
	batchId: string;
	albums: ManagedImportAlbumPreview[];
	decisions: Record<string, ManagedImportAlbumDecision>;
	isBusy: boolean;
	onChange: (decision: ManagedImportAlbumDecision) => void;
	onUploaded: () => Promise<void>;
	onUploadStateChange: (isUploading: boolean) => void;
};

export function ImportAlbumReview(props: Props) {
	if (props.albums.length === 0) return null;
	return (
		<div className="grid gap-4 px-4 py-4 sm:px-6">
			<div className="space-y-1">
				<h3 className="font-medium text-heading">Review albums and covers</h3>
				<p className="text-sm text-caption">
					Your file tags determine the album destination. Review the cover
					before importing. Nothing is added to your library until you confirm.
				</p>
			</div>
			{props.albums.map((album) => (
				<AlbumReview key={album.key} {...props} album={album} />
			))}
		</div>
	);
}

function AlbumReview({
	album,
	...props
}: Props & { album: ManagedImportAlbumPreview }) {
	const decision = props.decisions[album.key] ?? defaultAlbumDecision(album);
	const existing = decision.createSeparate
		? undefined
		: album.existingAlbums.find(
				(candidate) => candidate.id === decision.albumId,
			);
	const [error, setError] = useState("");
	const [isUploading, setIsUploading] = useState(false);
	const uploadId = useId();
	const isDisabled = props.isBusy || isUploading;
	async function upload(file: File | undefined) {
		if (!file) return;
		setIsUploading(true);
		props.onUploadStateChange(true);
		setError("");
		try {
			const artwork = await apiClient.uploadManagedImportArtwork(
				props.batchId,
				album.key,
				file,
			);
			await props.onUploaded();
			props.onChange({
				...decision,
				artworkMode: "selected",
				artworkId: artwork.id,
			});
		} catch (error) {
			setError(
				error instanceof Error ? error.message : "Album artwork upload failed",
			);
		} finally {
			setIsUploading(false);
			props.onUploadStateChange(false);
		}
	}
	return (
		<fieldset
			disabled={isDisabled}
			className="grid gap-4 rounded-lg border border-border bg-card/40 p-4 disabled:opacity-60"
		>
			<legend className="px-1 font-medium text-heading">
				{album.title} — {album.albumArtists.join(", ")}
			</legend>
			<AlbumDestination
				album={album}
				decision={decision}
				isDisabled={isDisabled}
				onChange={props.onChange}
			/>
			{existing?.hasArtwork ? (
				<p className="text-caption text-sm">
					Existing Album cover will be preserved.
				</p>
			) : (
				<>
					<ImportCoverPicker
						batchId={props.batchId}
						album={album}
						decision={decision}
						onChange={props.onChange}
					/>
					<div className="grid gap-2">
						<Label htmlFor={uploadId} className="font-normal leading-normal">
							Upload another cover · JPG or PNG, maximum 20 MiB
						</Label>
						<Input
							id={uploadId}
							aria-label={`Upload cover for ${album.title}`}
							type="file"
							accept=".jpg,.jpeg,.png,image/jpeg,image/png"
							onChange={(event) => {
								void upload(event.target.files?.[0]);
								event.target.value = "";
							}}
						/>
					</div>
					{existing ? (
						<p className="text-caption text-sm">
							Your import confirmation also approves adding this cover to the
							existing Album.
						</p>
					) : null}
				</>
			)}
			{isUploading ? <p aria-live="polite">Uploading artwork…</p> : null}
			{error ? (
				<p role="alert" className="text-destructive text-sm">
					{error}
				</p>
			) : null}
		</fieldset>
	);
}

function AlbumDestination({
	album,
	decision,
	isDisabled,
	onChange,
}: {
	album: ManagedImportAlbumPreview;
	decision: ManagedImportAlbumDecision;
	isDisabled: boolean;
	onChange: Props["onChange"];
}) {
	return (
		<div className="grid gap-2 text-sm">
			<p className="font-medium text-heading">Album destination</p>
			{album.existingAlbums.length === 0 ? (
				<div className="rounded-md border border-border bg-secondary/40 px-3 py-3">
					<p className="font-medium text-foreground">Create new album</p>
					<p className="mt-1 text-caption">
						No library album matches this title and album artist. A new album
						will be created when you confirm.
					</p>
				</div>
			) : (
				<>
					<Select
						disabled={isDisabled}
						items={[
							...album.existingAlbums.map((match) => ({
								value: match.id,
								label: existingAlbumLabel(match),
							})),
							{ value: "separate", label: "Create separate album" },
						]}
						value={
							decision.createSeparate ? "separate" : (decision.albumId ?? null)
						}
						onValueChange={(value) =>
							value &&
							onChange({
								...decision,
								createSeparate: value === "separate",
								albumId: value === "separate" ? undefined : value,
								artworkMode: value === "separate" ? "auto" : "none",
								artworkId: undefined,
							})
						}
					>
						<SelectTrigger
							aria-label={`Album destination for ${album.title}`}
							className="w-full bg-secondary/40 text-foreground"
						>
							<SelectValue placeholder="Choose an existing album" />
						</SelectTrigger>
						<SelectContent alignItemWithTrigger={false}>
							{album.existingAlbums.map((match) => (
								<SelectItem key={match.id} value={match.id}>
									{existingAlbumLabel(match)}
								</SelectItem>
							))}
							<SelectItem value="separate">Create separate album</SelectItem>
						</SelectContent>
					</Select>
					<p className="text-caption">
						Matches share this album title and album artist. Create a separate
						album to keep another edition.
					</p>
				</>
			)}
		</div>
	);
}

function existingAlbumLabel(
	match: ManagedImportAlbumPreview["existingAlbums"][number],
): string {
	return `Add to existing album${match.year ? ` (${match.year})` : ""} — ${match.tracks.length} tracks`;
}

export function ImportFileAlternatives({
	entries,
	isBusy,
	onSelect,
}: {
	entries: ImportFileEntry[];
	isBusy: boolean;
	onSelect: (key: string, selected: boolean) => void;
}) {
	const groups = new Map<string, ImportFileEntry[]>();
	for (const entry of entries) {
		if (entry.state !== "accepted" || !entry.preview) continue;
		const key = importPositionKey(entry);
		groups.set(key, [...(groups.get(key) ?? []), entry]);
	}
	return (
		<div className="grid gap-2 px-4 sm:px-6">
			{[...groups.entries()]
				.filter(
					([, files]) =>
						files.length > 1 &&
						new Set(
							files.map((file) => file.preview?.file.contentSha256 ?? file.key),
						).size > 1,
				)
				.map(([key, files]) => (
					<fieldset
						key={key}
						disabled={isBusy}
						className="rounded-sm border border-border p-3"
					>
						<legend className="px-1 text-sm">
							Choose one file for {files[0].preview?.file.album}, position{" "}
							{files[0].preview?.file.discNo}.{files[0].preview?.file.trackNo}
						</legend>
						<RadioGroup
							className="gap-0"
							disabled={isBusy}
							value={
								files.filter((file) => file.selected).length === 1
									? (files.find((file) => file.selected)?.key ?? null)
									: null
							}
							onValueChange={(entryKey) => onSelect(entryKey as string, true)}
						>
							{files.map((entry) => (
								<Label
									key={entry.key}
									className="py-1 font-normal leading-normal"
								>
									<RadioGroupItem value={entry.key} />
									{entry.file.name} — {entry.preview?.file.format.toUpperCase()}
									,{" "}
									{((entry.preview?.file.sizeBytes ?? 0) / 1024 / 1024).toFixed(
										1,
									)}{" "}
									MiB, {entry.preview?.file.sampleRateHz} Hz,{" "}
									{entry.preview &&
									"bitDepth" in entry.preview.file &&
									entry.preview.file.bitDepth
										? `${entry.preview.file.bitDepth}-bit, `
										: ""}
									{entry.preview?.file.bitrateKbps} kbps
								</Label>
							))}
						</RadioGroup>
					</fieldset>
				))}
		</div>
	);
}
