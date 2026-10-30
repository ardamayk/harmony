import type { ReactNode } from "react";
import {
	PAGE_CONTENT_PADDING_CLASS,
	PAGE_CONTENT_WIDTH_CLASS,
} from "#/lib/page-layout-classes";
import { cn } from "#/lib/utils";

/** The scroller of a {@link DetailPageShell}, for what follows its scrolling. */
export const DETAIL_SCROLLER_SELECTOR = "[data-detail-scroller]";

export function PageShell({
	testId,
	header,
	children,
	className,
	contentClassName,
	contentTestId,
}: {
	testId?: string;
	header: ReactNode;
	children: ReactNode;
	className?: string;
	contentClassName?: string;
	contentTestId?: string;
}) {
	return (
		<div
			data-testid={testId}
			className={cn("flex min-h-0 flex-1 flex-col overflow-hidden", className)}
		>
			{header}
			<div
				data-testid={contentTestId}
				className={cn(
					"min-h-0 flex-1 overflow-y-auto scrollbar-hidden",
					PAGE_CONTENT_PADDING_CLASS,
					contentClassName,
				)}
			>
				{children}
			</div>
		</div>
	);
}

export function PageHeader({
	title,
	actions,
	footer,
	className,
	innerClassName,
}: {
	title: string;
	actions?: ReactNode;
	footer?: ReactNode;
	className?: string;
	innerClassName?: string;
}) {
	// The open section is already lit in the Top Nav, so the page title stays
	// for screen readers only; a header with nothing visible collapses away.
	const hasVisibleContent = Boolean(actions || footer);
	return (
		<header
			className={cn(
				hasVisibleContent
					? "sticky top-0 z-40 shrink-0 bg-background/80 py-3 backdrop-blur"
					: "sr-only",
				PAGE_CONTENT_PADDING_CLASS.replace("py-6", ""),
				className,
			)}
		>
			<div className={cn("flex flex-col gap-2", innerClassName)}>
				<div className="flex items-start justify-between gap-3">
					<h1 className="sr-only">{title}</h1>
					{actions ? (
						<div className="ml-auto flex min-w-0 items-center justify-end gap-2">
							{actions}
						</div>
					) : null}
				</div>
				{footer}
			</div>
		</header>
	);
}

/**
 * Page body for a detail route (an album, playlist, genre or radio station).
 * Detail routes render straight into the app shell's scroll area rather than
 * through {@link PageShell}, so this applies the same padding and width the
 * list pages get and keeps the two aligned.
 */
export function DetailPageShell({
	testId,
	className,
	children,
}: {
	testId?: string;
	className?: string;
	children: ReactNode;
}) {
	return (
		// `page-edge-right` (styles.css) lets the scroller reach the window edge
		// behind the Queue Drawer, so a header backdrop can run full width.
		<div
			data-detail-scroller
			className="page-edge-right min-h-0 flex-1 overflow-y-auto scrollbar-hidden pl-(--shell-inset) py-6"
		>
			<div
				data-testid={testId}
				className={cn(PAGE_CONTENT_WIDTH_CLASS, className)}
			>
				{children}
			</div>
		</div>
	);
}
