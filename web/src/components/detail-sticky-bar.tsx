import type { LucideIcon } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { DETAIL_SCROLLER_SELECTOR } from "#/components/page-layout";
import { cn } from "#/lib/utils";

/**
 * Whether an element has scrolled out over the top of its detail page. An
 * element still waiting below the fold does not count, and without
 * IntersectionObserver the answer stays false.
 */
export function useScrolledPast() {
	const [element, setElement] = useState<HTMLElement | null>(null);
	const [scrolledPast, setScrolledPast] = useState(false);
	useEffect(() => {
		if (!element || typeof IntersectionObserver === "undefined") return;
		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					setScrolledPast(
						!entry.isIntersecting &&
							entry.boundingClientRect.top < (entry.rootBounds?.top ?? 0),
					);
				}
			},
			{ root: element.closest(DETAIL_SCROLLER_SELECTOR) },
		);
		observer.observe(element);
		return () => observer.disconnect();
	}, [element]);
	return [setElement, scrolledPast] as const;
}

/**
 * A detail page's compact header, shown while its hero's actions are scrolled
 * out of view, with the page blurred behind it. It goes first in the page
 * column: it takes no room there, and its edges bleed like the cover wash, so
 * its content stays on the column's lines (and on the track table's) with the
 * Queue Drawer open too.
 */
export function DetailStickyBar({
	visible,
	cover,
	title,
	subtitle,
	children,
}: {
	visible: boolean;
	cover: ReactNode;
	title: string;
	subtitle: ReactNode;
	/** `DetailStickyAction`s, on the bar's right. */
	children: ReactNode;
}) {
	if (!visible) return null;
	return (
		// `-top-6` undoes the scroller's `py-6`, which a sticky offset counts from.
		<div className="sticky -top-6 z-40 h-0">
			<div
				data-testid="detail-sticky-bar"
				className="-ml-(--shell-inset) -mr-(--page-edge-right) flex items-center gap-4 border-border border-b bg-background/30 py-3 phone:gap-2 phone:py-1 pr-(--page-edge-right) pl-(--shell-inset) backdrop-blur-xl backdrop-saturate-150 duration-150 motion-safe:animate-in motion-safe:fade-in"
			>
				<div className="contents phone:hidden">{cover}</div>
				<div className="min-w-0 flex-1">
					<p
						title={title}
						className="truncate font-semibold text-heading text-xl phone:text-base"
					>
						{title}
					</p>
					<p className="truncate text-caption text-sm">{subtitle}</p>
				</div>
				<div className="flex shrink-0 items-center gap-6 phone:gap-0">
					{children}
				</div>
			</div>
		</div>
	);
}

/** One of the bar's actions: an icon over its label, no outline. */
export function DetailStickyAction({
	icon: Icon,
	label,
	description = label,
	hint = description,
	filled = false,
	disabled,
	onClick,
}: {
	icon: LucideIcon;
	label: string;
	/** The accessible name, when the short label needs more. */
	description?: string;
	hint?: string;
	filled?: boolean;
	disabled?: boolean;
	onClick?: () => void;
}) {
	return (
		<button
			type="button"
			aria-label={description}
			title={hint}
			disabled={disabled}
			onClick={onClick}
			className="flex w-16 shrink-0 flex-col items-center gap-1 rounded-md py-1 font-medium text-heading text-xs transition enabled:hover:bg-muted focus-visible:bg-muted focus-visible:outline-none disabled:text-caption phone:w-11 phone:justify-center"
		>
			<Icon className={cn("size-6", filled && "fill-current")} />
			<span className="phone:hidden">{label}</span>
		</button>
	);
}
