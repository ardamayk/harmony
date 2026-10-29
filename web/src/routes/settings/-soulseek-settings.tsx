import {
	SOULSEEK_ACQUISITION_CAPABILITY,
	type SoulseekStatus,
} from "@repo/api-client";
import { useQuery } from "@tanstack/react-query";
import { useServerCapability } from "#/hooks/use-server-capability";
import { apiClient } from "#/lib/api";
import { soulseekErrorMessage, soulseekQueryKeys } from "#/lib/soulseek";

/**
 * Read-only state of the operator's slskd: whether it answers, whether it
 * is logged in to Soulseek, and whether it shares Managed Storage (Soulseek
 * Sharing). The operator changes all of it in slskd's configuration; there
 * is no switch here (ADR 0036).
 */
export function SoulseekSettingsSection() {
	const canAcquire = useServerCapability(SOULSEEK_ACQUISITION_CAPABILITY);
	const status = useQuery({
		queryKey: soulseekQueryKeys.status,
		queryFn: () => apiClient.getSoulseekStatus(),
		enabled: canAcquire,
	});
	if (!canAcquire) return null;
	return (
		<section aria-label="Soulseek" className="mb-8 flex flex-col gap-4">
			<h2 className="font-medium text-sm">Soulseek</h2>
			{status.data ? (
				<SoulseekStatusList status={status.data} />
			) : status.isError ? (
				<div role="alert" className="text-sm">
					<p className="text-destructive">
						{soulseekErrorMessage(status.error)}
					</p>
					<button
						type="button"
						className="mt-2 underline"
						onClick={() => void status.refetch()}
					>
						Retry
					</button>
				</div>
			) : (
				<p className="text-caption text-sm">Loading Soulseek status…</p>
			)}
			<p className="max-w-prose text-caption text-sm">
				The operator of this Music Server runs slskd and is responsible for what
				it downloads and shares. Downloading or sharing copyrighted music
				without permission is unlawful in many places.
			</p>
		</section>
	);
}

function SoulseekStatusList({ status }: { status: SoulseekStatus }) {
	const { slskd, soulseek, sharing } = status;
	return (
		<dl className="grid max-w-prose grid-cols-media gap-x-6 gap-y-2 text-sm">
			<dt className="text-caption">slskd</dt>
			<dd>
				{slskd.reachable
					? `Reachable${slskd.version ? ` · version ${slskd.version}` : ""}`
					: "Unreachable. Check that slskd is running and that its address and API key are configured."}
			</dd>
			<dt className="text-caption">Soulseek</dt>
			<dd>
				{!slskd.reachable
					? "Unknown"
					: soulseek.loggedIn
						? `Logged in${soulseek.username ? ` as ${soulseek.username}` : ""}`
						: soulseek.connected
							? "Connected, not logged in"
							: "Not connected"}
			</dd>
			<dt className="text-caption">Sharing</dt>
			<dd>
				{sharing.state === "shared" ? (
					`Your library is shared${
						sharing.sharedFileCount != null
							? ` · ${sharing.sharedFileCount.toLocaleString()} files`
							: ""
					}`
				) : sharing.state === "not_shared" ? (
					<>
						Your library is not shared.
						<span className="block text-caption">
							The operator shares Managed Storage in slskd’s configuration.
						</span>
					</>
				) : (
					"Unknown"
				)}
			</dd>
		</dl>
	);
}
