import type { ComponentProps, ReactNode } from "react";
import { Button } from "#/components/ui/button";
import {
	Empty,
	EmptyContent,
	EmptyDescription,
	EmptyHeader,
	EmptyMedia,
	EmptyTitle,
} from "#/components/ui/empty";
import { PAGE_CONTENT_WIDTH_CLASS } from "#/lib/page-layout-classes";
import { cn } from "#/lib/utils";

const SKELETON_CARD_KEYS = [
	"skeleton-1",
	"skeleton-2",
	"skeleton-3",
	"skeleton-4",
	"skeleton-5",
	"skeleton-6",
	"skeleton-7",
	"skeleton-8",
	"skeleton-9",
	"skeleton-10",
] as const;
export const COLLECTION_PAGE_CONTAINER_CLASS = PAGE_CONTENT_WIDTH_CLASS;
// Cards are exactly 250px wide and never stretch: when the column narrows
// (Queue Drawer opening, window shrinking) cards wrap to the next row instead
// of resizing. `min(…,100%)` lets one column fit narrow phones. Below `lg`
// an open Queue Drawer has a fixed width (see use-queue-drawer-width) and the
// grid stretches two columns into what is left.
export const COLLECTION_GRID_CLASS =
	"grid grid-cols-cards gap-3 max-lg:in-data-[queue-open]:grid-cols-2";

// Hover and keyboard focus for every collection and station card
// (docs/design-rules.md, Interaction states): lift, neutral heading-tone border, soft shadow;
// hover never takes the accent.
// `transform-gpu backface-hidden` keeps the desktop webview from flickering
// card edges while they move.
export const CARD_INTERACTION_CLASS =
	"transform-gpu backface-hidden outline-none transition duration-300 ease-out hover:-translate-y-1 hover:border-heading/35 hover:shadow-card-lift focus-visible:-translate-y-1 focus-visible:border-heading/60 focus-visible:shadow-card-lift motion-reduce:transform-none motion-reduce:transition-none";

export function CollectionPageContainer({
	className,
	...props
}: ComponentProps<"div">) {
	return (
		<div
			className={cn(COLLECTION_PAGE_CONTAINER_CLASS, className)}
			{...props}
		/>
	);
}

export function CollectionGrid({
	children,
	isBusy = false,
	className,
}: {
	children: ReactNode;
	isBusy?: boolean;
	className?: string;
}) {
	return (
		<div
			aria-busy={isBusy || undefined}
			className={cn(COLLECTION_GRID_CLASS, className)}
		>
			{children}
		</div>
	);
}

export function CollectionGridSkeleton({ label }: { label: string }) {
	return (
		<>
			<output className="sr-only" aria-live="polite">
				{label}
			</output>
			<CollectionGrid isBusy>
				{SKELETON_CARD_KEYS.map((key) => (
					<div
						key={key}
						data-testid="collection-card-skeleton"
						aria-hidden
						className="aspect-square animate-pulse rounded-md border border-border bg-muted/70 motion-reduce:animate-none"
					/>
				))}
			</CollectionGrid>
		</>
	);
}

export function CollectionGridState({
	kind,
	icon,
	title,
	description,
	onRetry,
	isRetrying = false,
}: {
	icon: ReactNode;
	title: string;
	description: string;
} & (
	| {
			kind: "empty";
			onRetry?: never;
			isRetrying?: never;
	  }
	| {
			kind: "error";
			onRetry: () => void;
			isRetrying?: boolean;
	  }
)) {
	const isError = kind === "error";

	return (
		<Empty
			role={isError ? "alert" : undefined}
			variant={isError ? "error" : "card"}
			className="min-h-72"
		>
			<EmptyHeader>
				<EmptyMedia variant="icon">{icon}</EmptyMedia>
				<EmptyTitle>{title}</EmptyTitle>
				<EmptyDescription>{description}</EmptyDescription>
			</EmptyHeader>
			{isError ? (
				<EmptyContent>
					<Button
						type="button"
						variant="outline"
						disabled={isRetrying}
						onClick={onRetry}
					>
						{isRetrying ? "Trying again…" : "Try again"}
					</Button>
				</EmptyContent>
			) : null}
		</Empty>
	);
}
