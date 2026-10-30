import { Menu, Tray, nativeImage } from "electron";

/** What the core sends as `shell:tray` (see playback_tray.rs). */
export type TrayUpdate = {
	sourceLabel: string;
	hasSource: boolean;
	previousLabel: string;
	toggleLabel: string;
	nextLabel: string;
	openLabel: string;
	miniLabel: string;
	quitLabel: string;
	/** Present only when the core decides the tooltip is due. */
	tooltip?: string;
};

/** Shell actions the core accepts from tray items (`shell_action`). */
export type TrayAction =
	| "open-main-window"
	| "toggle-play"
	| "previous"
	| "next"
	| "toggle-mini-player"
	| "quit";

export class PlaybackTray {
	private readonly tray: Tray;

	constructor(
		icon: string,
		private readonly onAction: (action: TrayAction) => void,
	) {
		this.tray = new Tray(nativeImage.createFromPath(icon));
		this.tray.setToolTip("Harmony");
		// Clicking the icon itself opens the window where the host allows it.
		this.tray.on("click", () => this.onAction("open-main-window"));
	}

	update(update: TrayUpdate): void {
		const action = (name: TrayAction) => () => this.onAction(name);
		this.tray.setContextMenu(
			Menu.buildFromTemplate([
				{ label: update.sourceLabel, enabled: false },
				{
					label: update.previousLabel,
					enabled: update.hasSource,
					click: action("previous"),
				},
				{
					label: update.toggleLabel,
					enabled: update.hasSource,
					click: action("toggle-play"),
				},
				{
					label: update.nextLabel,
					enabled: update.hasSource,
					click: action("next"),
				},
				{ type: "separator" },
				{ label: update.openLabel, click: action("open-main-window") },
				{ label: update.miniLabel, click: action("toggle-mini-player") },
				{ label: update.quitLabel, click: action("quit") },
			]),
		);
		if (update.tooltip !== undefined) this.tray.setToolTip(update.tooltip);
	}

	destroy(): void {
		this.tray.destroy();
	}
}
