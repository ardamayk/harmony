import { createContext, useContext } from "react";

// Keep context identity independent of the provider's Fast Refresh boundary.
export const ImportSessionContext = createContext<{ open: () => void } | null>(
	null,
);

export function useManagedImport() {
	const session = useContext(ImportSessionContext);
	if (!session)
		throw new Error("ImportSessionProvider is required for Managed Import");
	return session;
}
