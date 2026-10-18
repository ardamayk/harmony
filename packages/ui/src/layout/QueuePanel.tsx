import { ChevronDown, ListMusic, Pin, PinOff } from "lucide-react";
import { useRef, useState } from "react";
import { usePlayback } from "../playback/PlaybackProvider";
import { getQueuePanel } from "../widgets/layout-utils";
import { useLayout } from "./LayoutProvider";
import { PanelCollapseButton } from "./PanelCollapseButton";
import { useQueuePin } from "./QueuePinProvider";
import { QueueSections } from "./QueueSections";
import { findCurrentQueueIndex, getQueueMinutes } from "./queue-groups";

// Body text at rest, the hover step on hover and focus, and the shared lit look
// with accent glyph when pressed (Pin), as the Top Nav controls do.
const QUEUE_ICON_BUTTON_CLASS =
	"inline-flex size-8 items-center justify-center rounded-sm text-foreground transition hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:text-heading focus-visible:outline-none aria-pressed:bg-neutral-active aria-pressed:text-shell-active-foreground aria-pressed:shadow-lit";

export function QueuePanel({
	embedded = false,
	onClose,
}: {
	/** Embedded views omit the collapse control unless the host supplies one. */
	embedded?: boolean;
	onClose?: () => void;
} = {}) {
	const { preferences, togglePanel } = useLayout();
	const queuePin = useQueuePin();
	const scrollContainerRef = useRef<HTMLDivElement>(null);
	const [actionError, setActionError] = useState<string | null>(null);
	const [isUpdating, setIsUpdating] = useState(false);
	const panelSide = getQueuePanel(preferences.layout.sidebarPosition);
	const isCollapsed = !embedded && preferences.layout.collapsed[panelSide];
	const {
		queue,
		queueConflict,
		currentTrack,
		playbackSource,
		playQueueIndex,
		removeFromQueue,
		clearQueue,
		getAlbumCoverUrl,
		isPlaying,
	} = usePlayback();
	const currentIndex = findCurrentQueueIndex(
		queue,
		playbackSource?.type === "track" ? playbackSource.queueItemId : undefined,
		currentTrack?.id,
	);
	const upcoming = queue.slice(currentIndex + 1);
	// While a Track plays, Clear removes only what follows it.
	const hasClearableItems =
		currentIndex >= 0 ? upcoming.length > 0 : queue.length > 0;

	async function runAction(message: string, action: () => Promise<void>) {
		setActionError(null);
		setIsUpdating(true);
		try {
			await action();
		} catch (error) {
			console.warn(message, { error });
			setActionError(message);
		} finally {
			setIsUpdating(false);
		}
	}

	if (isCollapsed)
		return (
			<div className="flex h-full flex-col items-center bg-queue text-queue-foreground">
				<div className="flex w-full justify-center px-1 pt-2">
					<PanelCollapseButton
						edge={panelSide}
						collapsed
						onToggle={() => togglePanel(panelSide)}
					/>
				</div>
				<div
					className="relative mt-3 flex size-9 items-center justify-center rounded-sm text-caption"
					title={`Queue (${queue.length})`}
				>
					<ListMusic className="size-4" />
					{queue.length > 0 && (
						<span className="absolute -top-1 -right-1 flex size-4 items-center justify-center rounded-full bg-primary font-medium text-xs text-primary-foreground">
							{queue.length > 9 ? "9+" : queue.length}
						</span>
					)}
				</div>
			</div>
		);

	return (
		<div className="flex h-full min-h-0 flex-col">
			<div className="flex items-center justify-between gap-2 border-sidebar-border border-b pt-3 pr-3 pb-3 pl-4">
				<div className="min-w-0">
					<h2 className="font-semibold text-heading text-base leading-5.5">
						Queue
					</h2>
					<p className="whitespace-nowrap text-xs text-caption leading-4 tabular-nums">
						{upcoming.length} {upcoming.length === 1 ? "track" : "tracks"} ·{" "}
						{getQueueMinutes(upcoming)} min left
					</p>
				</div>
				<div className="flex shrink-0 items-center gap-1">
					<button
						type="button"
						className="h-8 rounded-sm border border-sidebar-border bg-player-pill px-3 text-foreground text-xs hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:text-heading focus-visible:outline-none disabled:pointer-events-none disabled:opacity-40"
						disabled={!hasClearableItems || isUpdating}
						onClick={() => void runAction("Failed to clear queue", clearQueue)}
					>
						Clear
					</button>
					{queuePin ? (
						<button
							type="button"
							aria-label="Pin queue"
							aria-pressed={queuePin.pinned}
							title={
								queuePin.pinned
									? "Unpin: float the queue over the page"
									: "Pin: make room for the queue beside the page"
							}
							className={QUEUE_ICON_BUTTON_CLASS}
							onClick={queuePin.togglePinned}
						>
							{queuePin.pinned ? (
								<Pin className="size-4" />
							) : (
								<PinOff className="size-4" />
							)}
						</button>
					) : null}
					{(onClose || !embedded) && (
						<button
							type="button"
							aria-label="Hide queue"
							title="Hide queue"
							className={QUEUE_ICON_BUTTON_CLASS}
							onClick={onClose ?? (() => togglePanel(panelSide))}
						>
							<ChevronDown className="size-4" />
						</button>
					)}
				</div>
			</div>
			<div
				ref={scrollContainerRef}
				className="min-h-0 flex-1 overflow-y-auto pt-2 pr-2 pb-3 pl-4 scrollbar-hidden"
			>
				{(queueConflict || actionError) && (
					<p
						role="alert"
						className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-destructive text-xs"
					>
						{queueConflict || actionError}
					</p>
				)}
				{queue.length === 0 ? (
					<p className="p-3 text-foreground text-sm">Queue is empty</p>
				) : (
					<QueueSections
						scrollContainerRef={scrollContainerRef}
						queue={queue}
						currentIndex={currentIndex}
						isPlaying={isPlaying}
						isUpdating={isUpdating}
						getAlbumCoverUrl={getAlbumCoverUrl}
						onPlay={(index) =>
							void runAction("Failed to play queue track", () =>
								playQueueIndex(index),
							)
						}
						onDismiss={(items) =>
							void runAction("Failed to remove queue suggestions", async () => {
								for (const item of items) await removeFromQueue(item.id);
							})
						}
					/>
				)}
			</div>
		</div>
	);
}
