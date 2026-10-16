import { X } from "lucide-react";
import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useFocusTrap } from "../lib/use-focus-trap";
import {
	describePlaybackShortcuts,
	type PlaybackKeyboardOptions,
} from "../playback/use-playback-keyboard-shortcuts";

/** Lists the player's keyboard shortcuts; opened with "?" or from the bar. */
export function ShortcutHelpOverlay({
	options,
	onClose,
}: {
	options?: PlaybackKeyboardOptions;
	onClose: () => void;
}) {
	const dialogRef = useRef<HTMLDivElement>(null);
	useFocusTrap(dialogRef);
	useEffect(() => {
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") onClose();
		};
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);

	if (typeof document === "undefined") return null;

	const shortcuts = describePlaybackShortcuts(options);

	return createPortal(
		<div className="fixed inset-0 z-[70] flex items-center justify-center bg-background/70 p-4">
			<div
				ref={dialogRef}
				role="dialog"
				aria-modal="true"
				aria-label="Keyboard shortcuts"
				tabIndex={-1}
				className="w-full max-w-md overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-xl"
			>
				<div className="flex items-center justify-between gap-3 border-border border-b p-4">
					<h2 className="font-semibold text-heading text-xl">
						Keyboard shortcuts
					</h2>
					<button
						type="button"
						className="inline-flex size-8 items-center justify-center rounded-full hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
						aria-label="Close"
						onClick={onClose}
					>
						<X className="size-4" />
					</button>
				</div>
				<dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-2 p-4 text-sm">
					{shortcuts.map((shortcut) => (
						<div key={shortcut.description} className="contents">
							<dt className="flex items-center gap-1">
								{shortcut.keys.map((key) => (
									<kbd
										key={key}
										className="inline-flex h-6 min-w-6 items-center justify-center rounded-sm border border-border bg-muted px-2 font-mono text-xs"
									>
										{key}
									</kbd>
								))}
							</dt>
							<dd className="text-foreground">{shortcut.description}</dd>
						</div>
					))}
				</dl>
			</div>
		</div>,
		document.body,
	);
}
