import {
	TopNavBackdrop,
	useCoverAccent,
	useLayout,
	useResolvedThemeMode,
} from "@repo/ui";
import { type CSSProperties, type ReactNode, useCallback, useRef } from "react";
import { DETAIL_SCROLLER_SELECTOR } from "#/components/page-layout";
import { cn } from "#/lib/utils";

const WASH_CLASS = "bg-linear-to-b from-(--cover-tint)/40 to-(--cover-tint)/0";
const TINT_STEP = "color-mix(in oklab, var(--cover-tint) 40%, transparent)";
const TINT_END = "color-mix(in oklab, var(--cover-tint) 0%, transparent)";

// The band's top step, then the band. Background layers and not a taller
// child: the Top Nav is the Desktop Client's drag region, a child inherits it
// past the clip, and every click over the page below would move the window.
const TOP_NAV_WASH_STYLE: CSSProperties = {
	backgroundImage: `linear-gradient(${TINT_STEP}, ${TINT_STEP}), linear-gradient(${TINT_STEP}, ${TINT_END})`,
	backgroundSize: "100% 100%, 100% var(--cover-wash-height, 0px)",
	backgroundPosition:
		"0 calc(0px - var(--cover-wash-scroll, 0px)), 0 calc(var(--cover-wash-top, 0px) - var(--cover-wash-scroll, 0px))",
	backgroundRepeat: "no-repeat",
};

/**
 * Keeps the Top Nav's copy of the band on the band: at the band's height and
 * scrolled with it, so the bar is a window onto the same surface.
 */
function keepWashOnBand(topNavWash: HTMLElement, band: HTMLElement | null) {
	const scroller = band?.closest<HTMLElement>(DETAIL_SCROLLER_SELECTOR);
	if (!band || !scroller) return;
	const follow = () => {
		topNavWash.style.setProperty(
			"--cover-wash-scroll",
			`${scroller.scrollTop}px`,
		);
	};
	const measure = () => {
		topNavWash.style.setProperty(
			"--cover-wash-top",
			`${topNavWash.offsetHeight}px`,
		);
		topNavWash.style.setProperty(
			"--cover-wash-height",
			`${band.getBoundingClientRect().height}px`,
		);
	};
	follow();
	measure();
	scroller.addEventListener("scroll", follow, { passive: true });
	const resizes =
		typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
	resizes?.observe(band);
	return () => {
		scroller.removeEventListener("scroll", follow);
		resizes?.disconnect();
	};
}

/**
 * A detail page header on a band in its image's colour. It bleeds to the
 * scroller's edges, behind the Queue Drawer too, and ends where the page's
 * list begins; no colour yet leaves the plain page background. The colour
 * runs up behind the Top Nav at the band's top step, then scrolls away with
 * the band.
 */
export function CoverWash({
	cacheKey,
	imageUrl,
	className,
	children,
}: {
	/** Shared with the Player Bar for Album covers, so a cover is read once. */
	cacheKey: string;
	imageUrl: string;
	/** Layout for what the band holds, such as an Artist photo laid on it. */
	className?: string;
	children: ReactNode;
}) {
	const { preferences } = useLayout();
	const accent = useCoverAccent({
		cacheKey,
		coverUrl: imageUrl,
		enabled: true,
		mode: useResolvedThemeMode(preferences.theme.mode),
	});
	const tint = accent["--cover-tint"];
	const tintStyle = { "--cover-tint": tint } as CSSProperties;
	const bandRef = useRef<HTMLDivElement>(null);
	const followBand = useCallback(
		(topNavWash: HTMLDivElement) => keepWashOnBand(topNavWash, bandRef.current),
		[],
	);
	return (
		<>
			<div
				ref={bandRef}
				className={cn(
					"-mt-6 -ml-(--shell-inset) -mr-(--page-edge-right) pt-6 pr-(--page-edge-right) pb-6 pl-(--shell-inset)",
					WASH_CLASS,
					className,
				)}
				style={tintStyle}
			>
				{children}
			</div>
			{/* After the band, whose ref `followBand` needs set by then. */}
			{tint ? (
				<TopNavBackdrop>
					<div
						ref={followBand}
						data-testid="top-nav-wash"
						className="absolute inset-0 bg-background"
						style={{ ...tintStyle, ...TOP_NAV_WASH_STYLE }}
					/>
				</TopNavBackdrop>
			) : null}
		</>
	);
}
