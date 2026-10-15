import type { ReactNode } from "react";
import {
	PAGE_CONTENT_PADDING_CLASS,
	PAGE_CONTENT_WIDTH_CLASS,
} from "#/lib/page-layout-classes";
import { cn } from "#/lib/utils";

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
					"min-h-0 flex-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
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
	return (
		<header
			className={cn(
				"sticky top-0 z-40 shrink-0 bg-background/80 py-3 backdrop-blur",
				PAGE_CONTENT_PADDING_CLASS.replace("py-5", ""),
				className,
			)}
		>
			<div className={cn("flex flex-col gap-2", innerClassName)}>
				<div className="flex items-start justify-between gap-3">
					<div className="min-w-0">
						<h1 className="font-semibold text-2xl text-heading tracking-normal">
							{title}
						</h1>
					</div>
					{actions ? (
						<div className="flex min-w-0 items-center justify-end gap-2">
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
		<div
			className={cn(
				"min-h-0 flex-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
				PAGE_CONTENT_PADDING_CLASS,
			)}
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
