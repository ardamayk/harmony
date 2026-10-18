import { useCoverAccent, useLayout, useResolvedThemeMode } from "@repo/ui";
import type { CSSProperties, ReactNode } from "react";

/**
 * A detail page header on a band in its image's colour. It bleeds to the
 * scroller's edges, behind the Queue Drawer too, and ends where the page's
 * list begins; no colour yet leaves the plain page background.
 */
export function CoverWash({
	cacheKey,
	imageUrl,
	children,
}: {
	/** Shared with the Player Bar for Album covers, so a cover is read once. */
	cacheKey: string;
	imageUrl: string;
	children: ReactNode;
}) {
	const { preferences } = useLayout();
	const accent = useCoverAccent({
		cacheKey,
		coverUrl: imageUrl,
		enabled: true,
		mode: useResolvedThemeMode(preferences.theme.mode),
	});
	return (
		<div
			className="-mt-6 -ml-(--shell-inset) -mr-(--page-edge-right) bg-linear-to-b from-(--cover-tint)/20 to-(--cover-tint)/5 pt-6 pr-(--page-edge-right) pb-6 pl-(--shell-inset)"
			style={{ "--cover-tint": accent["--cover-tint"] } as CSSProperties}
		>
			{children}
		</div>
	);
}
