import type {
	ManagedImportAlbumDecision,
	ManagedImportAlbumPreview,
} from "@repo/api-client";
import { Check, ImageOff } from "lucide-react";
import { useId, useState } from "react";
import { apiClient } from "#/lib/api";
import { cn } from "#/lib/utils";

export function ImportCoverPicker({
	batchId,
	album,
	decision,
	onChange,
}: {
	batchId: string;
	album: ManagedImportAlbumPreview;
	decision: ManagedImportAlbumDecision;
	onChange: (decision: ManagedImportAlbumDecision) => void;
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
			src: apiClient.getManagedImportArtworkUrl(batchId, artwork.id),
		})),
		{ id: "none", label: "No cover", src: undefined },
	];
	return (
		<fieldset className="grid gap-3">
			<legend className="mb-1 font-medium text-heading text-sm">
				Album cover
			</legend>
			<p className="text-caption text-sm">
				{album.artworks.length > 1
					? "Different covers were found. Choose one for this album, upload another, or continue without a cover."
					: album.artworks.length === 1
						? "One cover is available. Keep it, upload another image, or continue without a cover."
						: "No embedded cover was found. Upload an image or continue without a cover."}
			</p>
			<div className="flex flex-wrap gap-3">
				{options.map((option) => (
					<label
						key={option.id}
						className={cn(
							"relative grid w-32 cursor-pointer gap-2 rounded-md border bg-secondary/30 p-2 text-sm transition-colors focus-within:ring-2 focus-within:ring-ring",
							selectedId === option.id
								? "border-primary bg-primary/10"
								: "border-border hover:bg-secondary/60",
						)}
					>
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
						{option.src ? (
							<CoverPreview src={option.src} label={option.label} />
						) : (
							<div className="flex aspect-square w-full items-center justify-center rounded-md bg-secondary text-caption">
								<ImageOff className="size-8" aria-hidden="true" />
							</div>
						)}
						<span className="text-center font-medium text-foreground text-xs">
							{option.label}
						</span>
						{selectedId === option.id ? (
							<span className="absolute top-1 right-1 rounded-full bg-primary p-1 text-primary-foreground">
								<Check className="size-3" aria-hidden="true" />
								<span className="sr-only">Selected</span>
							</span>
						) : null}
					</label>
				))}
			</div>
		</fieldset>
	);
}

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
