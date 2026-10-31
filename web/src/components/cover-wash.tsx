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

/** Keeps header text at contrast on a bright cover, in either theme mode. */
const IMAGE_VEIL = "color-mix(in oklab, var(--background) 64%, transparent)";
const SCROLL = "var(--cover-wash-scroll, 0px)";
const BAND_HEIGHT = "var(--cover-wash-height, 0px)";
const BAND_TOP = `calc(var(--cover-wash-top, 0px) - ${SCROLL})`;

function imageLayer(imageUrl: string) {
	return `url(${JSON.stringify(imageUrl)})`;
}

// Sized and placed by `measure`, so the Top Nav's copy lines up. Past this
// width only a slice of the image would fit the band's height.
const IMAGE_MAX_WIDTH_REM = 48;
const IMAGE_WIDTH = "var(--cover-wash-image-width, 100%)";
const IMAGE_LEFT = "var(--cover-wash-image-left, 0px)";
/** Melts the image's left edge into the page, where the header's text sits. */
const IMAGE_EDGE = `linear-gradient(to right, var(--background) ${IMAGE_LEFT}, transparent calc(${IMAGE_LEFT} + 14rem))`;

function bandImageStyle(imageUrl: string): CSSProperties {
	return {
		backgroundImage: `linear-gradient(${TINT_STEP}, ${TINT_END}), linear-gradient(${IMAGE_VEIL}, var(--background)), ${IMAGE_EDGE}, ${imageLayer(imageUrl)}`,
		backgroundSize: `auto, auto, auto, ${IMAGE_WIDTH} auto`,
		backgroundPosition: `0 0, 0 0, 0 0, ${IMAGE_LEFT} 50%`,
		backgroundRepeat: "no-repeat",
	};
}

// The band's top step, then the band. Background layers and not a taller
// child: the Top Nav is the Desktop Client's drag region, a child inherits it
// past the clip, and every click over the page below would move the window.
function topNavWashStyle(imageUrl?: string): CSSProperties {
	const tint = {
		image: `linear-gradient(${TINT_STEP}, ${TINT_STEP}), linear-gradient(${TINT_STEP}, ${TINT_END})`,
		size: `100% 100%, 100% ${BAND_HEIGHT}`,
		position: `0 calc(0px - ${SCROLL}), 0 ${BAND_TOP}`,
	};
	if (!imageUrl) {
		return {
			backgroundImage: tint.image,
			backgroundSize: tint.size,
			backgroundPosition: tint.position,
			backgroundRepeat: "no-repeat",
		};
	}
	// The veil runs on past the band as the plain page, so the rest of the
	// image, taller than the band, never shows below it.
	return {
		backgroundImage: `${tint.image}, linear-gradient(${IMAGE_VEIL}, ${IMAGE_VEIL}), linear-gradient(${IMAGE_VEIL}, var(--background) ${BAND_HEIGHT}), ${IMAGE_EDGE}, ${imageLayer(imageUrl)}`,
		backgroundSize: `${tint.size}, 100% 100%, 100% calc(${BAND_HEIGHT} + ${IMAGE_WIDTH}), 100% 100%, ${IMAGE_WIDTH} auto`,
		backgroundPosition: `${tint.position}, 0 calc(0px - ${SCROLL}), 0 ${BAND_TOP}, 0 0, ${IMAGE_LEFT} calc(${BAND_TOP} + (${BAND_HEIGHT} - ${IMAGE_WIDTH}) / 2)`,
		backgroundRepeat: "no-repeat",
	};
}

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
		// The part of the band the Queue Drawer leaves in view.
		const { paddingLeft, paddingRight, fontSize } = getComputedStyle(band);
		const inView =
			band.getBoundingClientRect().width -
			Number.parseFloat(paddingRight) +
			Number.parseFloat(paddingLeft);
		const width = Math.min(
			inView,
			IMAGE_MAX_WIDTH_REM * Number.parseFloat(fontSize),
		);
		for (const surface of [topNavWash, band]) {
			surface.style.setProperty("--cover-wash-image-width", `${width}px`);
			surface.style.setProperty(
				"--cover-wash-image-left",
				`${inView - width}px`,
			);
		}
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
 * the band. With `backdropUrl`, that image lies under the colour at the
 * band's right, veiled and fading into the page.
 */
export function CoverWash({
	cacheKey,
	imageUrl,
	backdropUrl,
	className,
	children,
}: {
	/** Shared with the Player Bar for Album covers, so a cover is read once. */
	cacheKey: string;
	imageUrl: string;
	/** A larger copy than `imageUrl`, which is read only for its colour. */
	backdropUrl?: string;
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
	const bandImage = tint ? backdropUrl : undefined;
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
					!bandImage && WASH_CLASS,
					className,
				)}
				style={
					bandImage ? { ...tintStyle, ...bandImageStyle(bandImage) } : tintStyle
				}
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
						style={{ ...tintStyle, ...topNavWashStyle(bandImage) }}
					/>
				</TopNavBackdrop>
			) : null}
		</>
	);
}
