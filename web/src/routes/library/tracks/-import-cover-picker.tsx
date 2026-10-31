import type {
	ManagedImportAlbumDecision,
	ManagedImportAlbumPreview,
} from "@repo/api-client";
import { Check, ImageOff, ImagePlus } from "lucide-react";
import { useId, useState } from "react";
import { apiClient } from "#/lib/api";
import { cn } from "#/lib/utils";

export function ImportCoverPicker({
	batchId,
	album,
	decision,
	onChange,
	onUpload,
}: {
	batchId: string;
	album: ManagedImportAlbumPreview;
	decision: ManagedImportAlbumDecision;
	onChange: (decision: ManagedImportAlbumDecision) => void;
	onUpload: (file: File | undefined) => void;
}) {
	const groupId = useId();
	const selectedId =
		decision.artworkMode === "selected"
			? decision.artworkId
			: decision.artworkMode === "auto" && album.artworks.length === 1
				? album.artworks[0].id
				: decision.artworkMode === "none" || album.artworks.length === 0
					? "none"
					: undefined;
	const options = [
		...album.artworks.map((artwork, index) => ({
			id: artwork.id,
			label: artwork.jobId
				? `Embedded cover ${index + 1}`
				: `Uploaded cover ${index + 1}`,
			caption: artwork.jobId ? "Embedded" : "Uploaded",
			src: apiClient.getManagedImportArtworkUrl(batchId, artwork.id),
		})),
		{ id: "none", label: "No cover", caption: "No cover", src: undefined },
	];
	return (
		<fieldset className="grid gap-2">
			<legend className="sr-only">Album cover</legend>
			<div className="flex flex-wrap gap-2">
				{options.map((option) => (
					<label key={option.id} className={coverTileClassName}>
						<input
							type="radio"
							name={groupId}
							className="absolute inset-0 z-10 m-0 size-full cursor-pointer opacity-0 disabled:cursor-not-allowed"
							value={option.id}
							aria-label={`${option.label} for ${album.title}`}
							checked={selectedId === option.id}
							onChange={() =>
								onChange({
									...decision,
									artworkMode: option.id === "none" ? "none" : "selected",
									artworkId: option.id === "none" ? undefined : option.id,
								})
							}
						/>
						<span
							className={cn(
								"block overflow-hidden rounded-md ring-2 ring-offset-2 ring-offset-background transition-shadow",
								selectedId === option.id ? "ring-primary" : "ring-transparent",
							)}
						>
							{option.src ? (
								<CoverPreview src={option.src} label={option.label} />
							) : (
								<span className={coverPlaceholderClassName}>
									<ImageOff className="size-6" aria-hidden="true" />
								</span>
							)}
						</span>
						{option.caption}
						{selectedId === option.id ? (
							<span className="absolute top-1 right-1 rounded-full bg-primary p-1 text-primary-foreground">
								<Check className="size-3" aria-hidden="true" />
								<span className="sr-only">Selected</span>
							</span>
						) : null}
					</label>
				))}
				<label className={coverTileClassName}>
					<input
						type="file"
						className="absolute inset-0 z-10 m-0 size-full cursor-pointer opacity-0 disabled:cursor-not-allowed"
						aria-label={`Upload cover for ${album.title}`}
						accept=".jpg,.jpeg,.png,image/jpeg,image/png"
						onChange={(event) => {
							onUpload(event.target.files?.[0]);
							event.target.value = "";
						}}
					/>
					<span className={coverPlaceholderClassName}>
						<ImagePlus className="size-6" aria-hidden="true" />
					</span>
					Upload
				</label>
			</div>
			<p className="max-w-80 text-caption text-xs">
				{album.artworks.length > 1
					? "Different covers were found. Choose one, or upload a JPG or PNG up to 20 MiB."
					: album.artworks.length === 1
						? "Upload a JPG or PNG up to 20 MiB to use another cover."
						: "No embedded cover was found. Upload a JPG or PNG up to 20 MiB, or continue without a cover."}
			</p>
		</fieldset>
	);
}

const coverTileClassName =
	"relative grid w-24 cursor-pointer gap-1 rounded-md text-center text-caption text-xs transition-colors focus-within:ring-2 focus-within:ring-ring hover:text-heading";
const coverPlaceholderClassName =
	"flex aspect-square w-full items-center justify-center rounded-md bg-secondary";

function CoverPreview({ src, label }: { src: string; label: string }) {
	const [hasError, setHasError] = useState(false);
	if (hasError)
		return (
			<output className="flex aspect-square flex-col items-center justify-center gap-2 rounded-md bg-secondary px-2 text-center text-caption text-xs">
				<ImageOff className="size-6" aria-hidden="true" />
				Preview unavailable
			</output>
		);
	return (
		<img
			className="aspect-square w-full rounded-md object-cover"
			alt={label}
			src={src}
			onError={() => setHasError(true)}
		/>
	);
}
