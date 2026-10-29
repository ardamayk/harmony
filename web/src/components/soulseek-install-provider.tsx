import {
	type ExternalTrack,
	SOULSEEK_ACQUISITION_CAPABILITY,
} from "@repo/api-client";
import { type ReactNode, useCallback, useMemo, useState } from "react";
import { SoulseekInstallContext } from "#/components/soulseek-install-context";
import { SoulseekInstallDialog } from "#/components/soulseek-install-dialog";
import { useReturnFocus } from "#/hooks/use-return-focus";
import { useServerCapability } from "#/hooks/use-server-capability";

/**
 * Offers Install for External Tracks when the Music Server advertises
 * Soulseek Acquisition, and hosts the one Install dialog that the search
 * dialog and the Player Bar open.
 */
export function SoulseekInstallProvider({ children }: { children: ReactNode }) {
	const canInstall = useServerCapability(SOULSEEK_ACQUISITION_CAPABILITY);
	const [track, setTrack] = useState<ExternalTrack | null>(null);
	const [isOpen, setIsOpen] = useState(false);
	// The dialog opens from application state, so remember the opener.
	const { capture, restore } = useReturnFocus();
	const openInstall = useCallback(
		(next: ExternalTrack) => {
			capture();
			setTrack(next);
			setIsOpen(true);
		},
		[capture],
	);
	const value = useMemo(
		() => ({ canInstall, openInstall }),
		[canInstall, openInstall],
	);
	return (
		<SoulseekInstallContext.Provider value={value}>
			{children}
			{canInstall && track ? (
				<SoulseekInstallDialog
					// A new External Track starts a fresh dialog.
					key={track.id}
					track={track}
					open={isOpen}
					onOpenChange={setIsOpen}
					finalFocus={() => restore()}
				/>
			) : null}
		</SoulseekInstallContext.Provider>
	);
}
