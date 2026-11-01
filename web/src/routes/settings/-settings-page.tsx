import type { ThemePreferences } from "@repo/api-client";
import { useLayout } from "@repo/ui";
import { useHiddenFromGuest } from "@repo/ui/guest-access";
import { useQuery } from "@tanstack/react-query";
import type { CSSProperties } from "react";
import { Button } from "#/components/ui/button";
import { getDesktopMpvStatus, isDesktopClient } from "#/desktop/bridge";
import { apiClient } from "#/lib/api";
import { cn } from "#/lib/utils";
import { themePresetOptions } from "#/themes/presets";
import { DesktopConnectionSection } from "./-desktop-connection-settings";
import { ImportSettingsSection } from "./-import-settings";
import { PlaybackSettingsSection } from "./-playback-settings";
import { ServerDependenciesSection } from "./-server-dependencies";

const themeModes: ThemePreferences["mode"][] = ["light", "dark", "system"];

// Route components live outside the route file: with automatic code
// splitting, a component defined inside the route module can be rendered
// from its split chunk before that chunk finished evaluating after an HMR
// update in WebKit, which surfaces as "_s is not a function".
export function SettingsPage() {
	const { preferences, setPreferences } = useLayout();

	const health = useQuery({
		queryKey: ["health"],
		queryFn: () => apiClient.getHealth(),
	});

	const hideServerSettings = useHiddenFromGuest("server-settings");
	const hideImport = useHiddenFromGuest("managed-import");
	const isDesktop = isDesktopClient();
	const desktopMpv = useQuery({
		queryKey: ["desktop-mpv-status"],
		queryFn: () => getDesktopMpvStatus(),
		enabled: isDesktop,
	});

	const saveTheme = (theme: Partial<ThemePreferences>) => {
		const next = { ...preferences.theme, ...theme };
		setPreferences({ theme: next });
	};

	return (
		<div className="p-6">
			<header className="mb-6">
				<h1 className="font-semibold text-2xl">Settings</h1>
				{hideServerSettings ? (
					<p className="text-foreground text-sm">
						Appearance · Playback · saved on this device
					</p>
				) : (
					<p className="text-foreground text-sm">
						Appearance · Playback · Library · API {health.data?.status ?? "…"} v
						{health.data?.version ?? "…"}
					</p>
				)}
			</header>

			<section className="mb-8 flex flex-col gap-4">
				<h2 className="font-medium text-sm">Theme preset</h2>
				<div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3 phone:grid-cols-2">
					{themePresetOptions.map((preset) => {
						const isActive = preferences.theme.preset === preset.id;

						return (
							<button
								key={preset.id}
								type="button"
								onClick={() => saveTheme({ preset: preset.id })}
								className={cn(
									"rounded-lg border p-3 text-left transition",
									isActive
										? "border-primary bg-primary/5 ring-2 ring-primary/30"
										: "border-border bg-card/40 hover:bg-muted/40",
								)}
							>
								<div className="mb-3 flex h-10 overflow-hidden rounded-md border border-border/60">
									{preset.swatches.map((color) => (
										<span
											key={color}
											className="h-full flex-1 bg-(--swatch)"
											style={{ "--swatch": color } as CSSProperties}
										/>
									))}
								</div>
								<p className="font-medium text-heading text-sm">
									{preset.label}
								</p>
							</button>
						);
					})}
				</div>
			</section>

			<section className="mb-8 flex flex-col gap-4">
				<h2 className="font-medium text-sm">Appearance</h2>
				<div className="flex flex-wrap gap-2">
					{themeModes.map((mode) => (
						<Button
							key={mode}
							type="button"
							size="sm"
							variant={
								preferences.theme.mode === mode ? "default" : "secondary"
							}
							onClick={() => saveTheme({ mode })}
						>
							{mode}
						</Button>
					))}
				</div>
			</section>

			<PlaybackSettingsSection />

			{isDesktop ? <DesktopConnectionSection /> : null}

			{hideImport ? null : <ImportSettingsSection />}

			{hideServerSettings ? null : (
				<ServerDependenciesSection
					health={health.data}
					desktopMpv={
						desktopMpv.data ??
						(desktopMpv.isError
							? {
									available: false,
									pinned: false,
									pinnedVersion: "",
									detail: "status unavailable",
								}
							: null)
					}
				/>
			)}
		</div>
	);
}
