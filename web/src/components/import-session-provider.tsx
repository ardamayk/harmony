import { clearTrackWaveformCache } from "@repo/ui";
import { useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { ImportSessionContext } from "#/components/import-session-context";
import { useReturnFocus } from "#/hooks/use-return-focus";
import { invalidateLibraryCache } from "#/lib/invalidate-library-cache";
import { ImportMusicDialog } from "#/routes/library/tracks/-import-music-dialog";

export function ImportSessionProvider({ children }: { children: ReactNode }) {
	const session = useImportSession();
	return (
		<ImportSessionContext.Provider value={session}>
			{children}
			<ImportMusicDialog
				isOpen={session.isOpen}
				onOpenChange={session.handleOpenChange}
				onCommitted={session.refresh}
				finalFocus={session.restoreFocus}
			/>
		</ImportSessionContext.Provider>
	);
}

function useImportSession() {
	const [isOpen, setIsOpen] = useState(false);
	const queryClient = useQueryClient();
	// The dialog opens from the plus action without a DialogTrigger, so remember
	// the opener to restore focus on close.
	const returnFocus = useReturnFocus();
	async function refresh() {
		clearTrackWaveformCache();
		await invalidateLibraryCache(queryClient);
	}
	function handleOpenChange(nextIsOpen: boolean) {
		setIsOpen(nextIsOpen);
	}
	return {
		isOpen,
		open: () => {
			returnFocus.capture();
			setIsOpen(true);
		},
		handleOpenChange,
		refresh,
		restoreFocus: () => returnFocus.restore(),
	};
}
