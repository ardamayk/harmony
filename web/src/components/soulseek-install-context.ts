import type { ExternalTrack } from "@repo/api-client";
import { createContext, useContext } from "react";

export type SoulseekInstall = {
	/** True only while the Music Server advertises soulseek-acquisition.v1. */
	canInstall: boolean;
	/** Opens the Install dialog for an External Track. */
	openInstall: (track: ExternalTrack) => void;
};

// Keep context identity independent of the provider's Fast Refresh boundary.
// Without a provider, Install stays hidden.
export const SoulseekInstallContext = createContext<SoulseekInstall>({
	canInstall: false,
	openInstall: () => {},
});

export function useSoulseekInstall(): SoulseekInstall {
	return useContext(SoulseekInstallContext);
}
