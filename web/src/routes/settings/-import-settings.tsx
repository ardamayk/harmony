import { MANAGED_IMPORT_CAPABILITY } from "@repo/api-client";
import { useManagedImport } from "#/components/import-session-context";
import { Button } from "#/components/ui/button";
import { useServerCapabilityState } from "#/hooks/use-server-capability";

const IMPORT_UNSUPPORTED_TITLE =
	"This Music Server does not support Managed Import. Update the Music Server to import music.";

/** Entry point to Managed Import; the dialog itself is global (ImportSessionProvider). */
export function ImportSettingsSection() {
	const managedImport = useManagedImport();
	const importCapability = useServerCapabilityState(MANAGED_IMPORT_CAPABILITY);
	// Gate on the advertised Server Capability (ADR 0006). The control stays
	// usable while the health response is unknown so a slow answer never hides
	// the Import Music action from a compatible Music Server.
	const importUnsupported = importCapability === "missing";

	return (
		<section
			aria-labelledby="import-settings-heading"
			className="mb-8 flex flex-col gap-4"
		>
			<h2 id="import-settings-heading" className="font-medium text-sm">
				Library
			</h2>
			<p className="text-caption text-sm">
				Upload audio files or a local folder. Nothing is added to your library
				until you confirm.
			</p>
			<Button
				type="button"
				className="self-start"
				onClick={managedImport.open}
				disabled={importUnsupported}
				title={importUnsupported ? IMPORT_UNSUPPORTED_TITLE : undefined}
			>
				Import Music
			</Button>
		</section>
	);
}
