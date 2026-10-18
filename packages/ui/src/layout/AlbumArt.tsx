import { useEffect, useState } from "react";
import { cn } from "../lib/utils";

/** A failed image is asked for once more after this: a restarting Music Server is back by then. */
const RETRY_MS = 2000;

export function AlbumArt({
	coverUrl,
	title,
	className,
}: {
	coverUrl?: string | null;
	title: string;
	className?: string;
}) {
	// Failures belong to one URL, so a new cover (the Player Bar's next
	// track, a reused row) is tried afresh.
	const [failure, setFailure] = useState<{
		url: string;
		count: number;
		waiting: boolean;
	} | null>(null);
	const failedHere = failure !== null && failure.url === coverUrl;
	const showImage =
		coverUrl && !(failedHere && (failure.waiting || failure.count > 1));

	useEffect(() => {
		if (!failure?.waiting || failure.count > 1) return undefined;
		const timer = window.setTimeout(
			() => setFailure({ ...failure, waiting: false }),
			RETRY_MS,
		);
		return () => window.clearTimeout(timer);
	}, [failure]);

	if (showImage) {
		return (
			<img
				// A new key after a failure mounts a fresh image, which asks again.
				key={failedHere ? failure.count : 0}
				src={coverUrl}
				alt=""
				// A long grid asks only for what scrolls into view; an Artist
				// photo may still need its first online lookup.
				loading="lazy"
				className={cn("bg-muted object-cover", className)}
				onError={() =>
					setFailure((previous) => ({
						url: coverUrl,
						count: previous?.url === coverUrl ? previous.count + 1 : 1,
						waiting: true,
					}))
				}
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
