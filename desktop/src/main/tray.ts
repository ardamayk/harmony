import {
	Menu,
	type MenuItemConstructorOptions,
	Tray,
	nativeImage,
} from "electron";
import type { UpdateTrayItem } from "./updates";

/** What the core sends as `shell:tray` (see playback_tray.rs). */
export type TrayUpdate = {
	sourceLabel: string;
	hasSource: boolean;
	previousLabel: string;
	toggleLabel: string;
	nextLabel: string;
	openLabel: string;
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
	| "quit";

export class PlaybackTray {
	private readonly tray: Tray;
	private playback: TrayUpdate | null = null;
	private updateItem: UpdateTrayItem | null = null;

	constructor(
		icon: string,
		private readonly onAction: (action: TrayAction) => void,
		private readonly onUpdateAction: (
			action: NonNullable<UpdateTrayItem["action"]>,
		) => void,
	) {
		this.tray = new Tray(nativeImage.createFromPath(icon));
		this.tray.setToolTip("Harmony");
		// Clicking the icon itself opens the window where the host allows it.
		this.tray.on("click", () => this.onAction("open-main-window"));
	}

	update(update: TrayUpdate): void {
		this.playback = update;
		this.buildMenu(update);
		if (update.tooltip !== undefined) this.tray.setToolTip(update.tooltip);
	}

	/** Shows the update's pending action; null takes it off the menu. */
	setUpdateItem(item: UpdateTrayItem | null): void {
		this.updateItem = item;
		if (this.playback) this.buildMenu(this.playback);
	}

	destroy(): void {
		this.tray.destroy();
	}

	private updateMenuItems(): MenuItemConstructorOptions[] {
		if (!this.updateItem) return [];
		const { label, action } = this.updateItem;
		return [
			{ type: "separator" },
			action
				? { label, click: () => this.onUpdateAction(action) }
				: { label, enabled: false },
		];
	}

	private buildMenu(update: TrayUpdate) {
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
				...this.updateMenuItems(),
				{ type: "separator" },
				{ label: update.openLabel, click: action("open-main-window") },
				{ label: update.quitLabel, click: action("quit") },
			]),
		);
	}
}
