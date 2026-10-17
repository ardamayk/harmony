import type { HealthResponse } from "@repo/api-client";
import type { DesktopMpvStatus } from "#/desktop/bridge";

type Props = {
	health: HealthResponse | undefined;
	/** Pinned mpv sidecar status from the Desktop Client; null on the Web Client. */
	desktopMpv: DesktopMpvStatus | null;
};

type Row = {
	name: string;
	scope: "Music Server" | "Desktop Client";
	requirement: "Required" | "Optional";
	available: boolean;
	version?: string;
	detail?: string;
};

/**
 * Read-only view of the Server Dependencies probed by the Music Server and,
 * on the Desktop Client only, the pinned mpv sidecar.
 */
export function ServerDependenciesSection({ health, desktopMpv }: Props) {
	if (!health) {
		return (
			<section className="mb-8 flex flex-col gap-4">
				<h2 className="font-medium text-sm">Server dependencies</h2>
				<p className="text-muted-foreground text-sm">
					Loading server dependencies…
				</p>
			</section>
		);
	}

	const dependencies = health.dependencies ?? [];
	const rows: Row[] = dependencies.map((dependency) => ({
		name: dependency.name,
		scope: "Music Server",
		requirement: dependency.required ? "Required" : "Optional",
		available: dependency.available,
		version: dependency.version,
	}));
	if (desktopMpv) {
		rows.push({
			name: "mpv",
			scope: "Desktop Client",
			requirement: "Required",
			available: desktopMpv.available,
			version: desktopMpv.version ?? undefined,
			detail: desktopMpv.detail ?? undefined,
		});
	}

	return (
		<section className="mb-8 flex flex-col gap-4">
			<h2 className="font-medium text-sm">Server dependencies</h2>
			<div className="overflow-x-auto rounded-lg border border-border">
				<table className="w-full text-sm">
					<thead className="bg-muted/40 text-left text-muted-foreground">
						<tr>
							<th className="px-3 py-2 font-medium">Program</th>
							<th className="px-3 py-2 font-medium">Used by</th>
							<th className="px-3 py-2 font-medium">Requirement</th>
							<th className="px-3 py-2 font-medium">Status</th>
							<th className="px-3 py-2 font-medium">Version</th>
						</tr>
					</thead>
					<tbody>
						{rows.map((row) => (
							<tr key={row.name} className="border-border border-t">
								<td className="px-3 py-2 font-mono">{row.name}</td>
								<td className="px-3 py-2">{row.scope}</td>
								<td className="px-3 py-2">{row.requirement}</td>
								<td className="px-3 py-2">
									{row.available ? "Installed" : "Missing"}
									{row.detail ? ` · ${row.detail}` : null}
								</td>
								<td className="px-3 py-2">{row.version ?? "—"}</td>
							</tr>
						))}
					</tbody>
				</table>
			</div>
		</section>
	);
}
