import type {
	DesktopUpdateState,
	DesktopUpdateStatus,
} from "@repo/desktop-bridge";
import type { AppUpdater } from "electron-updater";

const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

export type Updater = Pick<
	AppUpdater,
	| "autoDownload"
	| "autoInstallOnAppQuit"
	| "checkForUpdates"
	| "downloadUpdate"
	| "on"
	| "quitAndInstall"
>;

export type UpdateControllerOptions = {
	currentVersion: string;
	/** Null where nothing can update: a client not packaged for release. */
	updater: Updater | null;
	onChange(status: DesktopUpdateStatus): void;
	/** Stops everything the installer would otherwise kill. */
	stopForInstall(): Promise<void>;
	/** The installer did not start, with the core already stopped. */
	onInstallFailed(): void;
};

/** What the tray offers for a state; no `action` shows it disabled. */
export type UpdateTrayItem = {
	label: string;
	action?: "download" | "install";
};

/**
 * The GitHub Releases named in the packaged app-update.yml are checked at start
 * and every six hours. Nothing else happens without a click: the user downloads
 * an update, then restarts into it (ADR 0039). A check nobody asked for that
 * fails is only logged and waits for the next one.
 */
export class UpdateController {
	private state: DesktopUpdateState;

	constructor(private readonly options: UpdateControllerOptions) {
		this.state = { status: options.updater ? "idle" : "unavailable" };
	}

	/** Starts the automatic checks. */
	start(): void {
		const { updater } = this.options;
		if (!updater) return;
		updater.autoDownload = false;
		updater.autoInstallOnAppQuit = false;
		updater.on("download-progress", ({ percent }) => {
			if (this.state.status !== "downloading") return;
			// Whole percents, so the tray menu is not rebuilt per chunk.
			const whole = Math.floor(percent);
			if (whole === this.state.percent) return;
			this.set({ ...this.state, percent: whole });
		});
		// An `error` event with no listener would crash the main process.
		updater.on("error", (error) => {
			process.stderr.write(`[shell] update failed: ${String(error)}\n`);
			if (this.state.status === "installing") this.options.onInstallFailed();
		});
		const check = () => void this.runCheck(false);
		check();
		setInterval(check, CHECK_INTERVAL_MS);
	}

	status(): DesktopUpdateStatus {
		return { currentVersion: this.options.currentVersion, ...this.state };
	}

	/** The user asked; a failure shows, unlike an automatic check's. */
	async check(): Promise<void> {
		await this.runCheck(true);
	}

	async download(): Promise<void> {
		const { updater } = this.options;
		const version = "version" in this.state ? this.state.version : undefined;
		const canDownload =
			this.state.status === "available" ||
			(this.state.status === "failed" && this.state.action === "download");
		if (!updater || !version || !canDownload) return;
		this.set({ status: "downloading", version, percent: 0 });
		try {
			await updater.downloadUpdate();
			this.set({ status: "downloaded", version });
		} catch {
			this.set({ status: "failed", action: "download", version });
		}
	}

	/**
	 * Restarts into the downloaded update. The core stops first: the installer
	 * kills whatever still runs from the install folder within a moment, and
	 * the Playback Session is saved only on the way out.
	 */
	async install(): Promise<void> {
		const { updater } = this.options;
		if (!updater || this.state.status !== "downloaded") return;
		this.set({ status: "installing", version: this.state.version });
		await this.options.stopForInstall();
		updater.quitAndInstall(true, true);
	}

	private async runCheck(asked: boolean): Promise<void> {
		const { updater } = this.options;
		if (!updater || this.isHoldingUpdate() || this.state.status === "checking")
			return;
		if (asked) this.set({ status: "checking" });
		try {
			const result = await updater.checkForUpdates();
			// A download the user started meanwhile outranks this answer.
			if (this.isHoldingUpdate()) return;
			this.set(
				result?.isUpdateAvailable
					? { status: "available", version: result.updateInfo.version }
					: { status: "up-to-date" },
			);
		} catch {
			if (asked) this.set({ status: "failed", action: "check" });
		}
	}

	/** Past Download, the offer stands until the restart; no check replaces it. */
	private isHoldingUpdate(): boolean {
		return (
			this.state.status === "downloading" ||
			this.state.status === "downloaded" ||
			this.state.status === "installing"
		);
	}

	private set(state: DesktopUpdateState) {
		this.state = state;
		this.options.onChange(this.status());
	}
}

/** The one pending action the tray mirrors; Settings holds everything else. */
export function updateTrayItem(
	state: DesktopUpdateState,
): UpdateTrayItem | null {
	switch (state.status) {
		case "available":
			return { label: `Download update ${state.version}`, action: "download" };
		case "downloading":
			return { label: `Downloading update ${state.percent}%` };
		case "downloaded":
			return { label: "Restart to update", action: "install" };
		case "installing":
			return { label: "Restarting to update" };
		case "failed":
			return state.action === "download"
				? { label: "Retry update download", action: "download" }
				: null;
		default:
			return null;
	}
}
