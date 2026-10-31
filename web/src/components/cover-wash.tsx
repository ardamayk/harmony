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

/** Dims the images; the left side, where header text sits, dims further. */
const IMAGE_VEIL = "color-mix(in oklab, var(--background) 50%, transparent)";
const TEXT_VEIL = "color-mix(in oklab, var(--background) 45%, transparent)";
const SCROLL = "var(--cover-wash-scroll, 0px)";
const BAND_HEIGHT = "var(--cover-wash-height, 0px)";
const BAND_TOP = `calc(var(--cover-wash-top, 0px) - ${SCROLL})`;

// Sized and placed by `measure`, so the Top Nav's copy lines up. The band is
// wide and short, so an image across all of it shows only a slice: a soft copy
// spans the content's edges, and the sharp one shows more of the image inside.
const SHARP_SHARE_OF_CONTENT = 0.65;
const CONTENT_LEFT = "var(--cover-wash-content-left, 0px)";
const CONTENT_WIDTH = "var(--cover-wash-content-width, 100%)";
const SHARP_LEFT = "var(--cover-wash-image-left, 0px)";
const SHARP_WIDTH = "var(--cover-wash-image-width, 100%)";

const TEXT_SIDE = `linear-gradient(to right, ${TEXT_VEIL} ${CONTENT_LEFT}, transparent calc(${CONTENT_LEFT} + ${CONTENT_WIDTH} * 0.5))`;

/** One image, centred on the band and faded towards its sides. */
function imageLayerStyle(
	url: string,
	left: string,
	width: string,
	bandTop: string,
	fade: number,
): CSSProperties {
	return {
		backgroundImage: `url(${JSON.stringify(url)})`,
		backgroundSize: `${width} auto`,
		backgroundPosition: `${left} calc(${bandTop} + (${BAND_HEIGHT} - ${width}) / 2)`,
		backgroundRepeat: "no-repeat",
		maskImage: `linear-gradient(to right, transparent ${left}, var(--background) calc(${left} + ${width} * ${fade}), var(--background) calc(${left} + ${width} * ${1 - fade}), transparent calc(${left} + ${width}))`,
	};
}

function CoverImages({
	softUrl,
	sharpUrl,
	bandTop,
}: {
	softUrl: string;
	sharpUrl: string;
	bandTop: string;
}) {
	return (
		<>
			<div
				className="absolute inset-0"
				style={imageLayerStyle(
					softUrl,
					CONTENT_LEFT,
					CONTENT_WIDTH,
					bandTop,
					0.05,
				)}
			/>
			<div
				className="absolute inset-0"
				style={imageLayerStyle(
					sharpUrl,
					SHARP_LEFT,
					SHARP_WIDTH,
					bandTop,
					0.22,
				)}
			/>
		</>
	);
}

const BAND_VEIL_STYLE: CSSProperties = {
	backgroundImage: `linear-gradient(${IMAGE_VEIL}, var(--background)), ${TEXT_SIDE}`,
};

// The veil runs on past the band as the plain page, so the rest of the image,
// taller than the band, never shows below it.
const TOP_NAV_VEIL_STYLE: CSSProperties = {
	backgroundImage: `linear-gradient(${IMAGE_VEIL}, ${IMAGE_VEIL}), linear-gradient(${IMAGE_VEIL}, var(--background) ${BAND_HEIGHT}), ${TEXT_SIDE}`,
	backgroundSize: `100% 100%, 100% calc(${BAND_HEIGHT} + ${CONTENT_WIDTH}), 100% 100%`,
	backgroundPosition: `0 calc(0px - ${SCROLL}), 0 ${BAND_TOP}, 0 0`,
	backgroundRepeat: "no-repeat",
};

// The band's top step, then the band, as background layers. Everything in the
// Top Nav stays the bar's own height: it is the Desktop Client's drag region,
// a taller child inherits it past the clip, and every click over the page
// below would move the window.
const TOP_NAV_TINT_STYLE: CSSProperties = {
	backgroundImage: `linear-gradient(${TINT_STEP}, ${TINT_STEP}), linear-gradient(${TINT_STEP}, ${TINT_END})`,
	backgroundSize: `100% 100%, 100% ${BAND_HEIGHT}`,
	backgroundPosition: `0 calc(0px - ${SCROLL}), 0 ${BAND_TOP}`,
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
		const bandBox = band.getBoundingClientRect();
		// Last, after the images when the band has them.
		const contentBox = (band.lastElementChild ?? band).getBoundingClientRect();
		const left = contentBox.left - bandBox.left;
		const sharpWidth = contentBox.width * SHARP_SHARE_OF_CONTENT;
		const measures = {
			"--cover-wash-height": bandBox.height,
			"--cover-wash-content-left": left,
			"--cover-wash-content-width": contentBox.width,
			"--cover-wash-image-left": left + (contentBox.width - sharpWidth) / 2,
			"--cover-wash-image-width": sharpWidth,
		};
		for (const surface of [topNavWash, band]) {
			for (const [name, pixels] of Object.entries(measures)) {
				surface.style.setProperty(name, `${pixels}px`);
			}
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
 * the band. With `backdropUrl`, the images replace the colour: veiled, across
 * what the band holds, and fading into the page.
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
	/** A larger copy; `imageUrl` is its soft copy as well as its colour. */
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
					bandImage ? "relative isolate" : WASH_CLASS,
					className,
				)}
				style={tintStyle}
			>
				{bandImage ? (
					<div
						aria-hidden
						className="pointer-events-none absolute inset-0 -z-10 overflow-hidden"
					>
						<CoverImages
							softUrl={imageUrl}
							sharpUrl={bandImage}
							bandTop="0px"
						/>
						<div className="absolute inset-0" style={BAND_VEIL_STYLE} />
					</div>
				) : null}
				{children}
			</div>
			{/* After the band, whose ref `followBand` needs set by then. */}
			{tint ? (
				<TopNavBackdrop>
					<div
						ref={followBand}
						data-testid="top-nav-wash"
						className="absolute inset-0 bg-background"
						style={
							bandImage ? tintStyle : { ...tintStyle, ...TOP_NAV_TINT_STYLE }
						}
					>
						{bandImage ? (
							<>
								<CoverImages
									softUrl={imageUrl}
									sharpUrl={bandImage}
									bandTop={BAND_TOP}
								/>
								<div className="absolute inset-0" style={TOP_NAV_VEIL_STYLE} />
							</>
						) : null}
					</div>
				</TopNavBackdrop>
			) : null}
		</>
	);
}
