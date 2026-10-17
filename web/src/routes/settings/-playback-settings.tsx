import type { PlaybackPreferences } from "@repo/api-client";
import { usePlaybackPreferences } from "@repo/ui";
import { Label } from "#/components/ui/label";
import { Switch } from "#/components/ui/switch";

const SEEK_STEP_OPTIONS = [5, 10, 15, 30, 60] as const;
const SEEK_STEP_LARGE_OPTIONS = [15, 30, 60, 120] as const;
const PLAYBACK_RATE_OPTIONS = [0.75, 1, 1.25, 1.5, 2] as const;
const TRANSITION_FADE_OPTIONS = [0, 150, 300, 600, 1000] as const;
const AUTO_SKIP_OPTIONS = [0, 3, 5, 10, 30] as const;

type ToggleKey = {
	[K in keyof PlaybackPreferences]: PlaybackPreferences[K] extends boolean
		? K
		: never;
}[keyof PlaybackPreferences];

const TOGGLES: Array<{ key: ToggleKey; label: string; description: string }> = [
	{
		key: "showWaveform",
		label: "Waveform seek bar",
		description:
			"Draw the track's loudness behind the seek bar. Needs ffmpeg on the Music Server.",
	},
	{
		key: "accentFromCover",
		label: "Accent from cover",
		description: "Tint the player controls with the album cover's colour.",
	},
	{
		key: "showUpNext",
		label: "Up next peek",
		description: "Show the next queued track beside the now-playing info.",
	},
	{
		key: "hoverTimestamp",
		label: "Hover timestamp",
		description: "Show the time under the pointer while hovering the seek bar.",
	},
];

function optionLabel(key: string, value: number): string {
	switch (key) {
		case "playbackRate":
			return `${value}×`;
		case "transitionFadeMs":
			return value === 0 ? "Off" : `${value} ms`;
		case "autoSkipOnErrorSeconds":
			return value === 0 ? "Wait for me" : `${value} s`;
		default:
			return `${value} s`;
	}
}

/**
 * Player Bar behaviour. Every change goes through LayoutProvider, which
 * persists Playback Preferences to the Music Server like theme and layout.
 */
export function PlaybackSettingsSection() {
	const { playback, setPlaybackPreferences } = usePlaybackPreferences();

	const choices: Array<{
		key:
			| "seekStepSeconds"
			| "seekStepLargeSeconds"
			| "playbackRate"
			| "transitionFadeMs"
			| "autoSkipOnErrorSeconds";
		label: string;
		description: string;
		options: readonly number[];
	}> = [
		{
			key: "seekStepSeconds",
			label: "Seek step",
			description: "How far the arrow keys move.",
			options: SEEK_STEP_OPTIONS,
		},
		{
			key: "seekStepLargeSeconds",
			label: "Large seek step",
			description: "How far Shift plus the arrow keys move.",
			options: SEEK_STEP_LARGE_OPTIONS,
		},
		{
			key: "playbackRate",
			label: "Default speed",
			description: "Applied to every track; pitch is preserved.",
			options: PLAYBACK_RATE_OPTIONS,
		},
		{
			key: "transitionFadeMs",
			label: "Transition fade",
			description:
				"Volume ramp when you pause, resume or skip. Tracks that follow each other naturally are never faded (desktop).",
			options: TRANSITION_FADE_OPTIONS,
		},
		{
			key: "autoSkipOnErrorSeconds",
			label: "Skip a failed track after",
			description: "Countdown before a track that cannot play is skipped.",
			options: AUTO_SKIP_OPTIONS,
		},
	];

	return (
		<section
			aria-labelledby="playback-settings-heading"
			className="mb-8 flex flex-col gap-4"
		>
			<h2 id="playback-settings-heading" className="font-medium text-sm">
				Playback
			</h2>
			<div className="grid gap-4 md:grid-cols-2">
				{choices.map((choice) => (
					<fieldset
						key={choice.key}
						className="m-0 rounded-lg border border-border bg-card/40 p-3"
					>
						<legend className="px-1 font-medium text-heading text-sm">
							{choice.label}
						</legend>
						<p className="mb-2 text-caption text-xs">{choice.description}</p>
						<div className="flex flex-wrap gap-2">
							{choice.options.map((option) => {
								const isActive = playback[choice.key] === option;
								return (
									<button
										key={option}
										type="button"
										aria-pressed={isActive}
										className={
											isActive
												? "rounded-sm border border-primary bg-primary/10 px-3 py-1 text-heading text-xs"
												: "rounded-sm border border-border px-3 py-1 text-xs hover:bg-muted/50"
										}
										onClick={() =>
											setPlaybackPreferences({ [choice.key]: option })
										}
									>
										{optionLabel(choice.key, option)}
									</button>
								);
							})}
						</div>
					</fieldset>
				))}
			</div>
			<ul className="grid gap-3 md:grid-cols-2">
				{TOGGLES.map((toggle) => (
					<li
						key={toggle.key}
						className="flex items-start justify-between gap-4 rounded-lg border border-border bg-card/40 p-3"
					>
						<div className="min-w-0">
							<Label
								htmlFor={`playback-${toggle.key}`}
								className="font-medium text-heading text-sm"
							>
								{toggle.label}
							</Label>
							<p className="text-caption text-xs">{toggle.description}</p>
						</div>
						<Switch
							id={`playback-${toggle.key}`}
							checked={playback[toggle.key]}
							onCheckedChange={(checked) =>
								setPlaybackPreferences({ [toggle.key]: checked })
							}
						/>
					</li>
				))}
			</ul>
		</section>
	);
}
