import { useState } from "react";
import { cn } from "../lib/utils";

export function AlbumArt({
	coverUrl,
	title,
	className,
}: {
	coverUrl?: string | null;
	title: string;
	className?: string;
}) {
	const [failed, setFailed] = useState(false);
	const showImage = coverUrl && !failed;

	if (showImage) {
		return (
			<img
				src={coverUrl}
				alt=""
				// A long grid asks only for what scrolls into view; an Artist
				// photo may still need its first online lookup.
				loading="lazy"
				className={cn("bg-muted object-cover", className)}
				onError={() => setFailed(true)}
			/>
		);
	}

	return (
		<div
			className={cn(
				"flex items-center justify-center bg-muted font-semibold uppercase text-caption",
				className,
			)}
			aria-hidden
		>
			{title.trim().slice(0, 1) || "♪"}
		</div>
	);
}
